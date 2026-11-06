use super::*;

#[test]
fn catalog_messages_match_built_in_translations() {
    assert_eq!(
        Messages::get("env.open_failed.windows.description", &[("file", ".env")]).unwrap(),
        "No se pudo abrir el archivo de entorno `.env` en la raíz del proyecto."
    );
    assert_eq!(
        Messages::get(
            "env.open_failed.windows.command",
            &[("file", ".env"), ("example", ".env.example")]
        )
        .unwrap(),
        "En PowerShell: Copy-Item -LiteralPath '.env.example' -Destination '.env'"
    );
    assert_eq!(
        Messages::get(
            "env.open_failed.linux.command",
            &[("example", ".env.example"), ("file", ".env")]
        )
        .unwrap(),
        "En la terminal: cp '.env.example' '.env'"
    );
}

#[test]
fn interpolation_preserves_parameter_bytes_without_reparsing() {
    assert_eq!(
        Messages::get(
            "env.open_failed.windows.action",
            &[("file", ".env"), ("example", "C:\\área\\:file.example")]
        )
        .unwrap(),
        "Verifica que `.env` exista y tenga permisos de lectura. Si falta, créalo desde `C:\\área\\:file.example`."
    );
}

#[test]
fn rejects_invalid_unknown_and_malformed_parameter_sets() {
    assert!(matches!(
        Messages::get("env..open_failed.windows.description", &[("file", ".env")]),
        Err(MessageError::InvalidKey(_))
    ));
    assert!(matches!(
        Messages::get("env.unknown", &[]),
        Err(MessageError::UnknownKey(_))
    ));
    assert!(matches!(
        Messages::get(
            "env.open_failed.windows.description",
            &[("filename", ".env")]
        ),
        Err(MessageError::MissingParameter { .. })
    ));
    assert!(matches!(
        Messages::get(
            "env.open_failed.windows.description",
            &[("file", "one"), ("file", "two")]
        ),
        Err(MessageError::DuplicateParameter(_))
    ));
    assert!(matches!(
        Messages::get(
            "env.open_failed.windows.description",
            &[("bad-name", ".env")]
        ),
        Err(MessageError::InvalidParameterName(_))
    ));
    assert!(matches!(
        Messages::get(
            "env.open_failed.windows.description",
            &[("unused", ".env"), ("file", ".env")]
        ),
        Err(MessageError::UnusedParameter { .. })
    ));
}
