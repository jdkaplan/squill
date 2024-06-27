use lazy_static::lazy_static;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};

use crate::MigrationId;

// These migration files either have no parameters (init) or will be modified before being run
// (new). The arguments to new migrations come from the same person who will be making those
// changes.
//
// So although configuring SQL escaping would be nice, I'm not worried about it for now.
//
// TODO: Call Tera::set_escape_fn to make me feel better.

lazy_static! {
    static ref TERA: Tera = {
        let mut tera = Tera::default();

        tera.add_raw_templates(vec![
            ("init.up.sql", include_str!("templates/init.up.sql")),
            ("init.down.sql", include_str!("templates/init.down.sql")),
            ("new.up.sql", include_str!("templates/new.up.sql")),
            ("new.down.sql", include_str!("templates/new.down.sql")),
        ])
        .expect("static templates");

        tera
    };
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TemplateId {
    InitUp,
    InitDown,
    NewUp,
    NewDown,
}

impl TemplateId {
    pub fn name(&self) -> &'static str {
        match self {
            TemplateId::InitUp => "init.up.sql",
            TemplateId::InitDown => "init.down.sql",
            TemplateId::NewUp => "new.up.sql",
            TemplateId::NewDown => "new.down.sql",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateContext {
    pub id: MigrationId,
    pub name: String,
}

impl TemplateContext {
    fn tera_context(&self) -> Context {
        let mut ctx = Context::new();
        ctx.insert("id", &self.id.as_i64());
        ctx.insert("name", &self.name);
        ctx
    }
}

#[derive(Debug, Clone)]
pub struct Templates {
    tera: Tera,
}

impl Templates {
    pub fn new(templates_dir: impl AsRef<Path>) -> Result<Self, TemplateError> {
        let templates_dir = templates_dir.as_ref();

        let mut templates = Self::default();

        for id in [TemplateId::NewUp, TemplateId::NewDown] {
            let path = templates_dir.join(id.name());

            if let Some(content) = read_file(&path)? {
                templates.register(id, &content)?;
            }
        }

        Ok(templates)
    }

    fn register(&mut self, id: TemplateId, content: &str) -> Result<(), TemplateError> {
        self.tera
            .add_raw_template(id.name(), content)
            .map_err(TemplateError::Parse)
    }

    pub fn render(&self, id: TemplateId, ctx: &TemplateContext) -> Result<String, TemplateError> {
        self.tera
            .render(id.name(), &ctx.tera_context())
            .map_err(TemplateError::Render)
    }
}

fn read_file(path: impl AsRef<Path>) -> Result<Option<String>, TemplateError> {
    let path = path.as_ref();
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),

        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),

        Err(err) => Err(TemplateError::Read {
            path: path.to_path_buf(),
            err,
        }),
    }
}

impl Default for Templates {
    fn default() -> Self {
        Self { tera: TERA.clone() }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TemplateError {
    #[error("failed to read template file: {path}: {err}")]
    Read { path: PathBuf, err: std::io::Error },

    #[error("failed to parse template file: {0}")]
    Parse(tera::Error),

    #[error("failed to render template: {0}")]
    Render(tera::Error),
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use super::*;

    #[tokio::test]
    async fn template_parse_error() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();
        let templates_dir = config.templates_dir.unwrap();

        std::fs::write(templates_dir.join("new.up.sql"), "Unmatched brace {{").unwrap();

        match Templates::new(&templates_dir) {
            Err(TemplateError::Parse(_)) => (),
            Ok(templates) => panic!("Templates built from invalid source file: {templates:?}"),
            Err(err) => panic!("{err:?}"),
        }
    }

    #[tokio::test]
    async fn no_template_overrides() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();
        let templates_dir = config.templates_dir.unwrap();

        let templates = Templates::new(templates_dir).unwrap();

        let ctx = TemplateContext {
            id: MigrationId(123),
            name: String::from("custom"),
        };

        for id in [TemplateId::NewUp, TemplateId::NewDown] {
            let expected = Templates::default().render(id, &ctx).unwrap();
            let actual = templates.render(id, &ctx).unwrap();
            assert_eq!(expected, actual);
        }
    }

    #[tokio::test]
    async fn custom_templates_only_one() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();
        let templates_dir = config.templates_dir.unwrap();

        std::fs::write(templates_dir.join("new.up.sql"), CUSTOM_UP).unwrap();

        let templates = Templates::new(templates_dir).unwrap();

        let ctx = TemplateContext {
            id: MigrationId(123),
            name: String::from("custom"),
        };

        let actual_up = templates.render(TemplateId::NewUp, &ctx).unwrap();
        let actual_down = templates.render(TemplateId::NewDown, &ctx).unwrap();

        let expected_up = r#"-- Up
-- 123 --
-- custom --
"#;

        let expected_down = Templates::default()
            .render(TemplateId::NewDown, &ctx)
            .unwrap();

        assert_eq!(expected_up, actual_up);
        assert_eq!(expected_down, actual_down);
    }

    #[tokio::test]
    async fn custom_templates_both() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();
        let templates_dir = config.templates_dir.unwrap();

        std::fs::write(templates_dir.join("new.up.sql"), CUSTOM_UP).unwrap();
        std::fs::write(templates_dir.join("new.down.sql"), CUSTOM_DOWN).unwrap();

        let templates = Templates::new(templates_dir).unwrap();

        let ctx = TemplateContext {
            id: MigrationId(123),
            name: String::from("custom"),
        };

        let actual_up = templates.render(TemplateId::NewUp, &ctx).unwrap();
        let actual_down = templates.render(TemplateId::NewDown, &ctx).unwrap();

        let expected_up = r#"-- Up
-- 123 --
-- custom --
"#;
        let expected_down = r#"/*
Down
123
custom
*/
"#;

        assert_eq!(expected_up, actual_up);
        assert_eq!(expected_down, actual_down);
    }
}
