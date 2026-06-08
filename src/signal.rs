use pipewire::channel;

pub struct Shutdown(());

pub fn shutdown() -> channel::Receiver<Shutdown> {
    let (tx, rx) = channel::channel::<Shutdown>();

    ctrlc::set_handler(move || {
        tx.send(Shutdown(())).ok();
    })
    .expect("Error setting Ctrl-C handler");

    rx
}
