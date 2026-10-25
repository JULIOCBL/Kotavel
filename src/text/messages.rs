use super::{MessageError, catalog};
use std::collections::HashMap;

pub type MessageParameter<'a> = (&'a str, &'a str);

pub struct Messages;

impl Messages {
    /// Looks up a built-in message and substitutes each named parameter once.
    ///
    /// Parameter values are copied literally and are never interpreted as part
    /// of the message template. Missing, duplicate, malformed or unused
    /// parameters return a typed error.
    pub fn get(key: &str, parameters: &[MessageParameter<'_>]) -> Result<String, MessageError> {
        if !catalog::valid_key(key) {
            return Err(MessageError::InvalidKey(key.to_owned()));
        }
        let template = catalog::get(key)?;
        let mut parameter_index = HashMap::with_capacity(parameters.len());
        for (index, (name, _)) in parameters.iter().enumerate() {
            if !valid_parameter_name(name) {
                return Err(MessageError::InvalidParameterName((*name).to_owned()));
            }
            if parameter_index.insert(*name, index).is_some() {
                return Err(MessageError::DuplicateParameter((*name).to_owned()));
            }
        }

        let mut used = vec![false; parameters.len()];
        let mut result = String::with_capacity(template.len());
        let bytes = template.as_bytes();
        let mut position = 0;
        while position < bytes.len() {
            if bytes[position] == b':'
                && position + 1 < bytes.len()
                && catalog::name_start(bytes[position + 1])
            {
                let start = position + 1;
                position = start + 1;
                while position < bytes.len() && catalog::name_part(bytes[position]) {
                    position += 1;
                }
                let name = &template[start..position];
                let Some(index) = parameter_index.get(name).copied() else {
                    return Err(MessageError::MissingParameter {
                        name: name.to_owned(),
                        key: key.to_owned(),
                    });
                };
                result.push_str(parameters[index].1);
                used[index] = true;
            } else {
                let character = template[position..]
                    .chars()
                    .next()
                    .expect("position is within the template");
                result.push(character);
                position += character.len_utf8();
            }
        }

        for (index, (name, _)) in parameters.iter().enumerate() {
            if !used[index] {
                return Err(MessageError::UnusedParameter {
                    name: (*name).to_owned(),
                    key: key.to_owned(),
                });
            }
        }
        Ok(result)
    }
}

fn valid_parameter_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(catalog::name_start) && bytes.all(catalog::name_part)
}

#[cfg(test)]
mod tests {
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
}
