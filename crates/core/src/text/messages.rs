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
#[path = "../../tests/unit/text/messages.rs"]
mod tests;
