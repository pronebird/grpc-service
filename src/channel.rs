use tokio::sync::{mpsc, oneshot};

pub struct Sender {
    sender: mpsc::Sender<oneshot::Sender<()>>,
}

impl Sender {
    pub async fn send(&self) {
        let (sender, receiver) = oneshot::channel();
        self.sender.send(sender).await.unwrap();
        receiver.await.unwrap();
    }
}

pub struct Receiver {
    receiver: mpsc::Receiver<oneshot::Sender<()>>,
}

impl Receiver {
    pub async fn recv(mut self) {
        let sender = self.receiver.recv().await.unwrap();
        sender.send(()).unwrap();
    }
}

pub fn channel() -> (Sender, Receiver) {
    let (sender, receiver) = mpsc::channel(1);
    (Sender { sender }, Receiver { receiver })
}
