#![warn(rust_2018_idioms)]
#![warn(clippy::all)]

mod progress;

use progress::ProgressDecorator;
use tokio::io::{self, BufReader, BufWriter};

#[tokio::main]
async fn main() {
    let pg_wtr = ProgressDecorator::new(io::sink());
    let pg = pg_wtr.progress();

    let download_handle = tokio::task::spawn(async move {
        let stream = tokio::fs::File::open("ubuntu.iso").await.unwrap();
        let (mut reader, mut writer) = (BufReader::new(stream), BufWriter::new(pg_wtr));
        io::copy(&mut reader, &mut writer).await.unwrap();
    });

    tokio::task::spawn(async move {
        loop {
            tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
            println!("{:?}", pg.to_size());
        }
    });

    download_handle.await.unwrap();
}
