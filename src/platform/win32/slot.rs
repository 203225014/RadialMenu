//! Состояние окна на потоке интерфейса.
//!
//! Правило владения в Win32-слое одно для всех окон:
//! * у каждого окна (приложение, заметка, настройки) своё состояние в своём `Slot`,
//!   живёт на главном потоке и доступно только из его оконных процедур;
//! * внутри `with` нельзя вызывать то, что синхронно шлёт сообщения своему же окну
//!   (SendMessage, DestroyWindow, SetFocus…): оконная процедура не получит состояние,
//!   `with` вернёт `None`. Такие вызовы делаются после выхода из `with`;
//! * поток хука мыши трогает только атомики `input::hook` и общается сообщениями.

use std::cell::RefCell;

pub struct Slot<T>(RefCell<Option<T>>);

impl<T> Slot<T> {
    pub const fn new() -> Slot<T> {
        Slot(RefCell::new(None))
    }

    /// Доступ к состоянию; `None` — окна нет или состояние уже занято выше по стеку.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.0.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f))
    }

    pub fn set(&self, v: T) {
        *self.0.borrow_mut() = Some(v);
    }

    pub fn take(&self) -> Option<T> {
        self.0.try_borrow_mut().ok().and_then(|mut g| g.take())
    }
}
