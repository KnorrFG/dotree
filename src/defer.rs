use core::mem::ManuallyDrop;

// shamelessly stolen from: https://github.com/andrewhickman/defer/blob/master/src/lib.rs

/// Defer execution of a closure until the return value is dropped.
pub fn defer<F>(f: F) -> impl Drop
where
    F: FnOnce(),
{
    struct Defer<F: FnOnce()>(ManuallyDrop<F>);

    impl<F: FnOnce()> Drop for Defer<F> {
        fn drop(&mut self) {
            let f: F = unsafe { ManuallyDrop::take(&mut self.0) };
            f();
        }
    }

    Defer(ManuallyDrop::new(f))
}

/// Defer execution of a closure until the current scope end.
#[macro_export]
macro_rules! defer {
    ($e:expr) => {
        let _defer = $crate::defer::defer(|| $e);
    };
}
