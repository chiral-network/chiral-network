use std::io::{self, Read};
use tokio;

mod dht;

fn main() {
    let mut node = dht::DHTNode::create().unwrap();

    // stuff to make this run async when you can't make the main function async
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.spawn(async move {
        node.start().await;
    });

    println!("press Enter to exit");
    let _ = io::stdin().read(&mut [0u8]).unwrap();
}
