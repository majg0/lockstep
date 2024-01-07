use std::mem::MaybeUninit;

pub trait DefaultArray: Default + Sized {
    fn default_array<const N: usize>() -> [Self; N] {
        // NOTE: we write directly, so it's fine
        let mut items: [MaybeUninit<Self>; N] = unsafe { MaybeUninit::uninit().assume_init() };

        for item in items.iter_mut() {
            *item = MaybeUninit::new(Default::default());
        }

        unsafe {
            // HACK: replace once https://github.com/rust-lang/rust/issues/96097 stabilizes
            (*(&MaybeUninit::new(items) as *const _ as *const MaybeUninit<_>)).assume_init_read()
        }
    }
}

impl<T: Default> DefaultArray for T {}
