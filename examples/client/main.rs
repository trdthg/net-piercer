mod config;
use bytes::BytesMut;
use config::Config;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
    sync::mpsc,
};

use log::{debug, error, info, warn};
use net_piercer::init_logger;
use uuid::Uuid;

fn main() {
    init_logger();
    let conf = config::read_configuration().expect("读取client.toml失败");
    let mut handles = Vec::new();
    // 依次尝试与server建立连接
    let mut handles = Vec::new();
    for client in &conf.client {
        let client = client.clone();
        let target_addr_str =
            conf.server_ip.clone() + ":" + client.remote_port.to_string().as_str();
        let target_addr: SocketAddr = target_addr_str.parse().unwrap();
        let main_addr_str = conf.server_ip.clone() + ":" + conf.server_port.to_string().as_str();
        let local_addr_str = client.local_ip.clone() + ":" + client.local_port.to_string().as_str();
        let local_addr: SocketAddr = local_addr_str.parse().unwrap();
        let main_addr = main_addr_str.parse().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok(resp) = register(
                main_addr,
                &client.name,
                &client.secret_key,
                &"tcp".to_string(),
            )
            .await
            {
                match resp {
                    RegisterResponse::Succ { uuid } => {
                        // 如果client注册成功，会得到一个uuid，每次向server发送数据时都带上这个uuid
                        let uuid_bin = bincode::serialize(&uuid).unwrap();
                        // 尝试与对应的建立连接，建立成功，就process
                        let conn = TcpSocket::new_v4().unwrap().connect(target_addr).await;
                        if let Ok(mut stream) = conn {
                            stream.write(uuid_bin.as_slice()).await.unwrap();
                            let mut buf = [0u8; 64];
                            let n = stream.read(&mut buf).await.unwrap();
                            let resp_str: String = bincode::deserialize(&buf[0..n]).unwrap();
                            if resp_str.eq("OK") {
                                process(stream, local_addr).await;
                            } else {
                                error!("Connect Failed!");
                            }
                        } else {
                            error!("Failed to connect target address:{}", target_addr_str);
                        }
                    }
                    RegisterResponse::Failed { reason } => {
                        error!("Failed to register: {}", reason);
                    }
                }
            }
        });
        handles.push(handle);
    }
    for h in handles {
        h.await.unwrap();
    }
}

pub async fn register(
    register_address: SocketAddr, // frps
    name: &String,                // name
    secret: &String,              // password
    protocol: &String,            // tcp
) -> Result<RegisterResponse, ()> {
    // 与frps建立连接
    let mut stream = if let Ok(socket) = TcpSocket::new_v4() {
        let stream = socket.connect(register_address).await;
        if let Err(e) = stream {
            error!(
                "Cannot connect to {} for register",
                register_address.to_string()
            );
            error!("{}", e.to_string());
            return Err(());
        }
        stream.unwrap()
    } else {
        error!("Cannot create TcpSocket");
        return Err(());
    };
    info!("Connect to {} for register.", register_address.to_string());
    let msg = ClientRegisterMessage {
        name: name.clone(),
        secret: secret.clone(),
        protocol: protocol.clone(),
    };
    let msg = bincode::serialize(&msg).unwrap();
    stream.write_all(msg.as_slice()).await.unwrap();

    let mut recv_buf = [0u8; 512];
    let size = stream.read(&mut recv_buf).await.unwrap();
    let resp: RegisterResponse = bincode::deserialize(&recv_buf[0..size]).unwrap();
    info!("Response: {:?}", resp);
    Ok(resp)
}

// 
pub async fn process(socket: TcpStream, local_addr: SocketAddr) {
    let (mut read, mut write) = socket.into_split();
    let (sender, mut receiver) = mpsc::unbounded_channel::<ForwardPackage>();
    let h1 = tokio::spawn(async move {
        let mut buf = BytesMut::with_capacity(128);
        let mut map: HashMap<Uuid, UnboundedSender<BytesMut>> = HashMap::new();
        loop {
            let re = read.read_buf(&mut buf).await;
            if re.is_err() || re.unwrap() == 0 {
                return;
            }
            while let Some(packet) = process_forward_bytes(&mut buf) {
                debug!("Receive packets from proxy server : {:?}", packet);
                match packet {
                    transport::ForwardPackage::Transport { uuid, data } => {
                        let (tx, mut rx) = mpsc::unbounded_channel();
                        if map.contains_key(&uuid) {
                            if let Err(e) = map.get(&uuid).unwrap().send(data) {
                                error!("forwarding data {} error", e);
                            }
                        } else {
                            map.insert(uuid, tx);
                            map.get(&uuid).unwrap().send(data).unwrap();
                            //establish the conn to local addr
                            let conn = TcpSocket::new_v4().unwrap().connect(local_addr).await;
                            if let Err(e) = conn {
                                error!(
                                    "Cannot establish the conn to addr:{}, {}",
                                    local_addr.to_string(),
                                    e
                                );
                                continue;
                            }
                            let (mut read, mut write) = conn.unwrap().into_split();
                            debug!("Connect to local: {} succ", local_addr);
                            tokio::spawn(async move {
                                while let Some(mut data) = rx.recv().await {
                                    while data.has_remaining() {
                                        info!("write data to:");
                                        write.write_buf(&mut data).await.unwrap();
                                    }
                                }
                            });
                            let sender = sender.clone();
                            tokio::spawn(async move {
                                loop {
                                    let mut buf = BytesMut::with_capacity(512);
                                    match read.read_buf(&mut buf).await {
                                        Ok(n) => {
                                            if n != 0 {
                                                sender
                                                    .send(ForwardPackage::Transport {
                                                        uuid,
                                                        data: buf,
                                                    })
                                                    .unwrap();
                                            } else {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            error!("{}", e);
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                    }
                    transport::ForwardPackage::HeartBeat { state } => {
                        panic!("Unimplemented! {}", state);
                    }
                }
            }
        }
    });
    let h2 = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Some(package) => {
                    debug!("Send the package: {:?}", package);
                    let bin = bincode::serialize(&package).unwrap();
                    let packet_length = bin.len();
                    //send the packet magic number (java .class magic number), and the data's length
                    write.write_u32(0xCAFEBABE).await.unwrap(); //TODO: handle the error here
                    write.write_u32(packet_length as u32).await.unwrap();
                    if let Ok(_) = write.write_all(bin.as_slice()).await {
                    } else {
                        break;
                    };
                }
                None => {
                    break;
                }
            }
        }
    });
    h1.await.unwrap();
    h2.await.unwrap();
}

pub fn process_forward_bytes(buf: &mut BytesMut) -> Option<ForwardPackage> {
    if !buf.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        drop_bytes_until_magic_number(buf);
        return None;
    } else {
        if buf.len() > 8 {
            let size = buf.get(4..8).unwrap();
            let size = size.clone().get_u32();
            if buf.len() >= (size + 8) as usize {
                let mut ret = buf.split_to(size as usize + 8);
                ret.advance(8);
                if let Ok(packet) = bincode::deserialize::<ForwardPackage>(ret.chunk()) {
                    return Some(packet);
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
}
fn drop_bytes_until_magic_number(buf: &mut BytesMut) {
    for i in 0..buf.len() {
        if buf[i].eq(&0xCAu8) {
            buf.advance(i);
        }
    }
}
