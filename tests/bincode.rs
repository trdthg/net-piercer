use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Entity {
    // x: u32,
    // y: f64,
    z: String,
}

#[derive(Serialize, Deserialize)]
struct Array(Vec<Entity>);

#[test]
fn bin() {
    let config = bincode::DefaultOptions::new();
    let world = vec![
        Entity {
            // x: 1,
            // y: 2.0,
            z: "aaa".to_string(),
        },
        // Entity {
        //     x: 2,
        //     y: 4.0,
        //     z: "bbbbb".to_string(),
        // },
        // Entity {
        //     x: 1,
        //     y: 2.0,
        //     z: "ccccc".to_string(),
        // },
    ];

    let encoded = bincode::serialize(&world).expect("编码失败");
    // 结构体: 8
    // x: 3 * 4(u32) = 12
    // y: 3 * 8(f64) = 24 36 81 - 36 = 45
    // z: (3 + 5 + 5) + 8 * 3= 37
    println!("{}", encoded.len());
    let a = Array(world);
    let encoded = bincode::serialize(&a).expect("编码失败2");
    println!("{}", encoded.len());

    let encoded = bincode::serialize(&1).unwrap();
    println!("{}", encoded.len());
}
