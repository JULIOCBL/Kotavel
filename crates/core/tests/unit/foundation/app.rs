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
