use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// Add this function before main()
pub async fn start_health_server() {
    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
    println!("Health check server running on port 8080");

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer).await;

                let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }
}
