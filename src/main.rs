#![warn(rust_2018_idioms)]
#![warn(clippy::all)]

mod progress;

use progress::Progress;
use tokio::io::{self, BufReader, BufWriter};

#[tokio::main]
async fn main() {
    let pg = Progress::new(io::sink());
    let pg_clone = pg.clone();

    let download_handle = tokio::task::spawn(async move {
        let stream = tokio::fs::File::open("ubuntu.iso").await.unwrap();
        let (mut reader, mut writer) = (BufReader::new(stream), BufWriter::new(pg_clone));
        io::copy(&mut reader, &mut writer).await.unwrap();
    });

    tokio::task::spawn(async move {
        loop {
            tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
            let size = pg.to_size();
            println!("{:?}", size.await);
        }
    });

    download_handle.await.unwrap();
}
