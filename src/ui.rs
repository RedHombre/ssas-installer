//! Log condiviso tra i thread di installazione e l'interfaccia grafica.

use std::sync::mpsc::Sender;
use std::sync::Mutex;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub fn err<T>(msg: impl Into<String>) -> Res<T> {
    Err(msg.into().into())
}

#[derive(Clone, Copy, PartialEq)]
pub enum Lvl {
    Step,
    Ok,
    Warn,
    Info,
    Cmd,
    Out,
    Err,
}

pub enum Msg {
    Line(Lvl, String),
    /// Fine dell'installazione: esito, messaggio e (url, token) del proxy appena installato.
    Done { ok: bool, text: String, proxy: Option<(String, String)> },
}

static SINK: Mutex<Option<Sender<Msg>>> = Mutex::new(None);
static REPAINT: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

pub fn set_sink(tx: Sender<Msg>, repaint: Box<dyn Fn() + Send>) {
    *SINK.lock().unwrap() = Some(tx);
    *REPAINT.lock().unwrap() = Some(repaint);
}

pub fn clear_sink() {
    *SINK.lock().unwrap() = None;
    *REPAINT.lock().unwrap() = None;
}

pub fn send(m: Msg) {
    if let Some(tx) = SINK.lock().unwrap().as_ref() {
        let _ = tx.send(m);
    }
    if let Some(r) = REPAINT.lock().unwrap().as_ref() {
        r();
    }
}

fn emit(l: Lvl, t: &str) {
    send(Msg::Line(l, t.to_string()));
}

pub fn step(t: &str) {
    emit(Lvl::Step, t)
}
pub fn ok(t: &str) {
    emit(Lvl::Ok, t)
}
pub fn warn(t: &str) {
    emit(Lvl::Warn, t)
}
pub fn info(t: &str) {
    emit(Lvl::Info, t)
}
pub fn cmd_line(t: &str) {
    emit(Lvl::Cmd, t)
}
pub fn out(t: &str) {
    emit(Lvl::Out, t)
}
