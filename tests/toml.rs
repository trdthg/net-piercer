#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Debug, Serialize, PartialEq)]
    struct Config {
        ip: String,
        port: Option<u32>,
        keys: Keys,
        vec: Vec<Keys>,
    }

    #[derive(Deserialize, Debug, Serialize, PartialEq)]
    struct Keys {
        username: String,
        password: String,
    }

    #[test]
    fn basic() {
        let s = r#"
            ip = '127.0.0.1'

            [keys]
            username = 'root'
            password = '000000'

            [[vec]]
            username = 'root'
            password = '000000'
            [[vec]]
            username = 'root'
            password = '000000'
        "#;

        let config: Config = toml::from_str(s).unwrap();
        println!("{:#?}", config);
        assert_eq!(config.ip, "127.0.0.1");

        let ser = toml::to_string(&config).unwrap();
        let der: Config = toml::from_str(&ser).unwrap();
        assert_eq!(der, config);
    }
}
