# Net Piercer

一个由rust实现的内网穿透工具，第一版基本完成

## 介绍

本项目代码量不多，适合练习rust语言，socket网络编程，channel，异步相关

## todo

- [ ] feat: 增加对命令行启动的支持
- [ ] feat: 支持多种日志写入方式 `Options::new().to_file("default.log").to_console(true)`
- [ ] feat: deseralize package from buf 可以优化为迭代器
- [ ] feat: buffer的read_frame优化封装

- [ ] bug: client取消连接，server端的fake server依然占据着端口

- [x] feat: 协议尾部标识(可以不要，因为头部含有数据长度)