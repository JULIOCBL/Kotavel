use super::{CauseChain, ContextField, DiagnosticContext, ReportStatus, TraceOrigin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Production,
    Development,
}

#[derive(Clone, Debug)]
pub struct ErrorDetails {
    pub code: String,
    pub operation: String,
    pub message: String,
    pub context: DiagnosticContext,
    pub recoverable: bool,
    pub module: String,
    pub fields: Vec<ContextField>,
    pub chain: CauseChain,
    pub report_status: ReportStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedError {
    pub message: String,
    pub reference: String,
    pub detail: String,
    pub exit_code: i32,
}

impl RenderedError {
    pub fn console_text(&self) -> String {
        format!(
            "{}\nReferencia: {}\n{}",
            self.message, self.reference, self.detail
        )
    }
}

pub fn render_error(
    error: &ErrorDetails,
    environment: Environment,
    public_message: Option<&str>,
) -> RenderedError {
    let mut result = RenderedError {
        message: console_safe(
            public_message
                .filter(|message| !message.is_empty())
                .unwrap_or("No se pudo completar la operación."),
        ),
        reference: console_safe(&error.operation),
        detail: String::new(),
        exit_code: 1,
    };
    if environment == Environment::Development {
        result.detail = format!(
            "Código: {}\nCausa: {}\n",
            console_safe(&error.code),
            console_safe(&error.message)
        );
        if error.context.origin == TraceOrigin::ExceptionConstruction {
            if let Some(location) = error.context.location {
                result.detail.push_str(&format!(
                    "Excepción construida en: {}:{}\n",
                    console_safe(location.file_name()),
                    location.line()
                ));
            }
        } else {
            result
                .detail
                .push_str("Origen original no disponible; traza del manejador.\n");
        }
        result
            .detail
            .push_str(&format!("Módulo: {}\n", console_safe(&error.module)));
        for field in &error.fields {
            result.detail.push_str(&format!(
                "{}: {}\n",
                console_safe(&field.key),
                console_safe(&field.value)
            ));
        }
        result.detail.push_str(&error.context.stack.format());
        for cause in &error.chain.causes {
            result.detail.push_str(&format!(
                "\nCausa anidada: {}\n",
                console_safe(&cause.message)
            ));
            if let Some(code) = &cause.code {
                result
                    .detail
                    .push_str(&format!("Código: {}\n", console_safe(code)));
            }
        }
        if error.chain.truncated {
            result.detail.push_str("\nCadena de causas truncada.\n");
        }
        if error.report_status == ReportStatus::Failed {
            result
                .detail
                .push_str("\nNo se pudo escribir el diagnóstico.\n");
        }
    }
    result
}

fn console_safe(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}
