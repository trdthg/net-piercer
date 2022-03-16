use bytes::{BufMut, Bytes, BytesMut};
#[test]
fn basic_usage() {
    let mut bytes = BytesMut::with_capacity(1024);

    bytes.put(&b"abcde"[..]);
    assert_eq!(bytes, b"abcde"[..]);

    // 16位占两个字节
    bytes.put_u16(15);
    assert_eq!(bytes, b"abcde\0\x0f"[..]);

    // split一下，原bytes空了🦊
    let a = bytes.split();
    assert_eq!(a, b"abcde\0\x0f"[..]);
    assert_eq!(bytes, b""[..]);

    bytes.put_u16(16);
    println!("{bytes:?}")
}
