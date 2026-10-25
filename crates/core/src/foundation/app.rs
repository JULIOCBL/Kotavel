use std::{cell::RefCell, marker::PhantomData, rc::Rc};

thread_local! {
    static ACTIVE_LOCALE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Error returned when a locale tag does not match Kotavel's portable format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocaleError {
    InvalidTag,
}

impl std::fmt::Display for LocaleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTag => {
                f.write_str("Locale must be a valid language tag (1–64 ASCII characters)")
            }
        }
    }
}

impl std::error::Error for LocaleError {}

/// Lightweight facade. Locale overrides belong to the current execution thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct App;

impl App {
    pub fn set_locale(&self, locale: &str) -> Result<(), LocaleError> {
        validate_locale(locale)?;
        ACTIVE_LOCALE.with(|active| *active.borrow_mut() = Some(locale.to_owned()));
        Ok(())
    }

    pub fn current_locale(&self) -> String {
        ACTIVE_LOCALE.with(|active| active.borrow().as_deref().unwrap_or("en").to_owned())
    }

    pub fn is_locale(&self, locale: &str) -> bool {
        self.current_locale() == locale
    }
}

/// Process-wide facade; per-thread state is isolated inside `App` methods.
pub fn app() -> &'static App {
    static APP: App = App;
    &APP
}

/// Temporarily set a locale for an operation and restore its previous value on drop.
pub struct AppLocaleScope {
    previous: Option<String>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl AppLocaleScope {
    pub fn new(default_locale: &str) -> Result<Self, LocaleError> {
        validate_locale(default_locale)?;
        let previous = ACTIVE_LOCALE.with(|active| active.replace(Some(default_locale.to_owned())));
        Ok(Self {
            previous,
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for AppLocaleScope {
    fn drop(&mut self) {
        ACTIVE_LOCALE.with(|active| *active.borrow_mut() = self.previous.take());
    }
}

fn validate_locale(locale: &str) -> Result<(), LocaleError> {
    if locale.is_empty() || locale.len() > 64 {
        return Err(LocaleError::InvalidTag);
    }
    let mut segment_length = 0;
    for byte in locale.bytes() {
        if byte == b'-' || byte == b'_' {
            if segment_length == 0 || segment_length > 8 {
                return Err(LocaleError::InvalidTag);
            }
            segment_length = 0;
        } else if byte.is_ascii_alphanumeric() {
            segment_length += 1;
            if segment_length > 8 {
                return Err(LocaleError::InvalidTag);
            }
        } else {
            return Err(LocaleError::InvalidTag);
        }
    }
    if segment_length == 0 {
        return Err(LocaleError::InvalidTag);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_scopes_nest_restore_and_validate_tags() {
        assert_eq!(app().current_locale(), "en");
        {
            let _outer = AppLocaleScope::new("es-MX").unwrap();
            assert!(app().is_locale("es-MX"));
            {
                let _inner = AppLocaleScope::new("fr_CA").unwrap();
                assert!(app().is_locale("fr_CA"));
            }
            assert!(app().is_locale("es-MX"));
        }
        assert!(app().is_locale("en"));
        for invalid in ["", "-en", "en-", "en--US", "longsegment9", "es MX", "es.MX"] {
            assert_eq!(
                app().set_locale(invalid),
                Err(LocaleError::InvalidTag),
                "{invalid}"
            );
        }
    }

    #[test]
    fn locale_is_thread_local() {
        let _scope = AppLocaleScope::new("es").unwrap();
        let child = std::thread::spawn(|| app().current_locale());
        assert_eq!(child.join().unwrap(), "en");
        assert_eq!(app().current_locale(), "es");
    }
}
