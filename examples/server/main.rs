mod config;
use config::{ClientConfig, Config};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use log::{debug, error, info, warn};
use net_piercer::init_logger;
use uuid::Uuid;

/// server
/// 从client接受数据，然后转发到内网中
#[tokio::main]
async fn main() {
    // 初始化配置文件，日志
    init_logger();
    let conf = config::read_server_configuration().expect("读取配置文件失败");

    // 绑定端口
    let addr = conf.host.clone() + ":" + conf.port.to_string().as_str();
    let listener = TcpListener::bind(addr).await.unwrap();
    info!("Server bind on address: {}", addr);

    // 保存现有的client连接
    let connection_state = Arc::new(Mutex::new(HashMap::<String, ConnectionState>::new()));

    // 等待client连接
    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        info!("Receive Connection from {}", addr);

        // 检查client是否通过
        let mut recv_buf = [0u8; 512];
        let size = stream.read(&mut recv_buf).await.unwrap();
        let client: ClientRegisterMessage = bincode::deserialize(&recv_buf[0..size]).unwrap();
        info!("Received the register information: {:?}", client);

        // 校验用户是否是server.toml中配置的有效用户
        if let Some(find) = conf
            .client
            .iter()
            .find(|x| x.name == client.name && x.secret_key == client.secret)
        {
            // 如果是，就下一步
            tokio::spawn(async move {
                process(stream, addr, find, connection_state.clone()).await;
            });
        } else {
            info!("Client Register Failed: {:?}", client);
            let response = RegisterResponse::Failed {
                reason: "Invalid register configuration.".to_string(),
            };
            let b = bincode::serialize(&response).unwrap();
            match stream.write(b.as_slice()).await {
                Ok(e) => {
                    debug!("Write register failed info, size of bytes: {}", e);
                }
                Err(e) => {
                    error!("Write Register Response Error: {}", e);
                }
            }
        }
    }
}

pub async fn process(
    mut socket: TcpStream,
    addr: SocketAddr,
    conf: Config,
    db: Arc<Mutex<HashMap<String, ConnectionState>>>,
) {
    // 生成一个uuid，向db中写入用户信息，向用户返回该uuid
    let uuid = Uuid::new_v4();
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards");
    db.lock().await.insert(
        uuid.to_string(),
        ConnectionState {
            last_heart_beat: 0, //TODO:
            register_time: since_the_epoch.as_millis(),
            name: client.name,
        },
    );
    info!(
        "Client named {} register successfully, uuid: {}",
        client_config.name,
        uuid.to_string()
    );
    // 尝试与client建立反向长连接，forward_listener用于等待
    let response = RegisterResponse::Succ {
        uuid: uuid.to_string(),
    };
    let b = bincode::serialize(&response).unwrap();

    let forward_addr = conf.host + ":" + find.port.to_string().as_str();
    let forward_listener = TcpListener::bind(&forward_addr).await;
    if let Err(e) = forward_listener {
        error!("Failed to bind on {} to forward data: {}", forward_addr, e);
        return;
    }
    let forward_listener = forward_listener.unwrap();
    info!("Listen on {} to forward data", forward_addr.to_string());
    // 如果连接成功，就
    match stream.write(b.as_slice()).await {
        Ok(e) => {
            debug!("Write register succ info, size of bytes: {}", e);
        }
        Err(e) => {
            error!("Write Register Response Error: {}", e);
        }
    }
    // 长连接建立完成，准备转发数据,
    loop {
        let (st, ad) = forward_listener.accept().await.unwrap();
        let mut forward_socket = st;
        // 校验客户端ip, 是否能read, 是否能读取到uuid,是否uuid相同
        if !ad.ip().eq(&addr.ip()) {
            forward_socket
                .write(b"Service Unavaliable.\n")
                .await
                .unwrap();
            forward_socket.shutdown().await.unwrap();
        }
        let mut buf = [0u8; 128];
        let re = forward_socket.read(&mut buf).await;
        if re.is_err() {
            forward_socket
                .write(b"Service Unavaliable.\n")
                .await
                .unwrap();
            forward_socket.shutdown().await.unwrap();
        }
        let receive_uuid_str: String = bincode::deserialize(&buf[0..re.unwrap()]).unwrap();
        let receive_uuid = Uuid::parse_str(receive_uuid_str.as_str());
        if receive_uuid.is_err() {
            info!(
                "Client send uuid: {} isn't match {}",
                receive_uuid_str,
                uuid.to_string()
            );
            break;
        }
        let ruuid = receive_uuid.unwrap();
        if ruuid != uuid {
            info!(
                "Client send uuid: {} isn't match {}",
                receive_uuid_str,
                uuid.to_string()
            );
            break;
        }
        forward_socket
            .write_all(bincode::serialize("OK").unwrap().as_ref())
            .await
            .unwrap();
        handle_forawrd_connection(forward_socket, forward_listener).await;
    }
}

async fn handle_forawrd_connection(
    forward_stream: TcpStream,     // 与client的连接
    forward_listener: TcpListener, // 在公网上，监听公网用户连接
) {
    let map: HashMap<Uuid, UnboundedSender<BytesMut>> = HashMap::new();
    let map = Arc::new(Mutex::new(map));

    // 用户和server的通信
    let (sender, mut receiver) = mpsc::unbounded_channel::<ForwardPackage>();

    // server和user的通信
    let (mut read, mut write) = forward_stream.into_split();

    // 如果接收到用户请求(receriver.recv())，就打包好并转发到client(writer.write())
    tokio::spawn(async move {
        loop {
            while let Some(package) = receiver.recv().await {
                debug!("Send the package: {:?}", package);
                let bin = bincode::serialize(&package).unwrap();
                let packet_length = bin.len();
                //send the packet magic number (java .class magic number), and the data's length
                write.write_u32(0xCAFEBABE).await.unwrap();
                write.write_u32(packet_length as u32).await.unwrap();
                if let Ok(_) = write.write_all(bin.as_slice()).await {
                } else {
                    break;
                };
            }
        }
    });

    let mm = map.clone();

    // 尝试从client读取数据，读取到的话，就解析，并根据uuid判断这个数据包是那个用户的, 然后向对应的user返回数据
    // 所以client和server只有1个连接
    tokio::spawn(async move {
        let mut buf = BytesMut::with_capacity(128);
        let map = mm;
        loop {
            let re = read.read_buf(&mut buf).await;
            if re.is_err() || re.unwrap() == 0 {
                break;
            }
            while let Some(packet) = process_forward_bytes(&mut buf) {
                debug!("Receive the packet {:?}", packet);
                match packet {
                    ForwardPackage::Transport { uuid, data } => {
                        if let Some(e) = map.lock().await.get(&uuid) {
                            if let Err(e) = e.send(data) {
                                error!("{}", e);
                            };
                        } else {
                            error!("Cannot find the receiver");
                        }
                    }
                    ForwardPackage::HeartBeat { state } => {
                        panic!("Unimplemented! {}", state);
                    }
                }
            }
        }
    });

    //
    loop {
        let (stream, _addr) = forward_listener.accept().await.unwrap();
        // stream是user，sender是向server发送
        handle_transport(stream, map.clone(), sender.clone()).await;
    }
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

async fn handle_transport(
    stream: TcpStream,
    map: Arc<Mutex<HashMap<Uuid, UnboundedSender<BytesMut>>>>,
    sender: UnboundedSender<ForwardPackage>,
) {
    // 当user第一次访问时，给他一个uuid，并向map中插入该用户
    let uuid = Uuid::new_v4();

    let (mut read, mut write) = stream.into_split();

    // 之后用户发送数据是，就会读取512字节，加上uuid，组成这个用户的数据包，封装完成后统一通过channel依次写入client
    tokio::spawn(async move {
        let uuid = uuid.clone();
        let mut buf = [0u8; 512];
        loop {
            match read.read(&mut buf).await {
                Ok(n) => {
                    if n != 0 {
                        let mut bytes = BytesMut::with_capacity(512);
                        bytes.put(&buf[0..n]);
                        let packet = ForwardPackage::Transport { uuid, data: bytes };
                        sender.send(packet).unwrap();
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    map.lock().await.insert(uuid, tx);
    // 如果从rx接收到数据，表示有数据包被解析完成
    tokio::spawn(async move {
        while let Some(mut data) = rx.recv().await {
            while data.has_remaining() {
                write.write_buf(&mut data).await.unwrap();
            }
        }
    });
}
