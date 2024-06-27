use lazy_static::lazy_static;
use std::borrow::Borrow;
use std::ffi::OsString;
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TemplateGroup {
    #[default]
    Default,
    Named(String),
}

impl TemplateGroup {
    fn join(&self, id: TemplateId) -> String {
        match self {
            TemplateGroup::Named(name) => format!("{}/{}", name, id.name()),
            TemplateGroup::Default => id.name().to_owned(),
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

        // The default template is in the directory root.
        templates.register_group(TemplateGroup::Default, templates_dir)?;

        // Named templates are in subdirectories.
        for subdir in named_template_dirs(templates_dir)? {
            let name = subdir.file_name().expect("directory has name");

            // Tera needs the template "path" to be a str
            let Some(name) = name.to_str() else {
                return Err(TemplateError::DirName(TemplateDirNameError::NotUtf8 {
                    name: name.to_owned(),
                }));
            };

            templates.register_group(TemplateGroup::Named(name.to_owned()), &subdir)?;
        }

        Ok(templates)
    }

    fn register_group(&mut self, group: TemplateGroup, dir: &Path) -> Result<(), TemplateError> {
        for id in [TemplateId::NewUp, TemplateId::NewDown] {
            let path = dir.join(id.name());

            if let Some(content) = read_file(&path)? {
                self.register(&group, id, &content)?;
            }
        }

        Ok(())
    }

    fn register(
        &mut self,
        group: &TemplateGroup,
        id: TemplateId,
        content: &str,
    ) -> Result<(), TemplateError> {
        self.tera
            .add_raw_template(&group.join(id), content)
            .map_err(TemplateError::Parse)
    }

    pub fn render(
        &self,
        group: impl Borrow<TemplateGroup>,
        id: TemplateId,
        ctx: &TemplateContext,
    ) -> Result<String, TemplateError> {
        let group = group.borrow();

        self.tera
            .render(&group.join(id), &ctx.tera_context())
            .map_err(TemplateError::Render)
    }
}

fn read_file(path: impl AsRef<Path>) -> Result<Option<String>, TemplateReadError> {
    let path = path.as_ref();
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),

        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),

        Err(err) => Err(TemplateReadError {
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
    #[error(transparent)]
    ReadDir(#[from] TemplateDirError),

    #[error(transparent)]
    DirName(#[from] TemplateDirNameError),

    #[error(transparent)]
    ReadFile(#[from] TemplateReadError),

    #[error("failed to parse template file: {0}")]
    Parse(tera::Error),

    #[error("failed to render template: {0}")]
    Render(tera::Error),
}

#[derive(thiserror::Error, Debug)]
#[error("failed to read template directory: {path}: {err}")]
pub struct TemplateDirError {
    path: PathBuf,
    err: std::io::Error,
}

#[derive(thiserror::Error, Debug)]
pub enum TemplateDirNameError {
    #[error("directory name is not UTF-8: {name:?}")]
    NotUtf8 { name: OsString },
}

#[derive(thiserror::Error, Debug)]
#[error("failed to read template file: {path}: {err}")]
pub struct TemplateReadError {
    path: PathBuf,
    err: std::io::Error,
}

fn named_template_dirs(dir: &Path) -> Result<Vec<PathBuf>, TemplateDirError> {
    let entries = match dir.read_dir() {
        Ok(entries) => entries,

        // Avoid a useless error if the directory doesn't exist.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }

        Err(err) => {
            return Err(TemplateDirError {
                path: dir.to_path_buf(),
                err,
            });
        }
    };

    let paths: Vec<PathBuf> = entries
        .filter_map(|entry| {
            let Ok(path) = entry.as_ref().map(|e| e.path()) else {
                tracing::debug!("skipping directory entry error: {:?}", entry);
                return None;
            };

            path.is_dir().then_some(path)
        })
        .collect();

    Ok(paths)
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
            let expected = Templates::default()
                .render(TemplateGroup::Default, id, &ctx)
                .unwrap();
            let actual = templates.render(TemplateGroup::Default, id, &ctx).unwrap();
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

        let actual_up = templates
            .render(TemplateGroup::Default, TemplateId::NewUp, &ctx)
            .unwrap();
        let actual_down = templates
            .render(TemplateGroup::Default, TemplateId::NewDown, &ctx)
            .unwrap();

        let expected_up = r#"-- Up
-- 123 --
-- custom --
"#;

        let expected_down = Templates::default()
            .render(TemplateGroup::Default, TemplateId::NewDown, &ctx)
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

        let actual_up = templates
            .render(TemplateGroup::Default, TemplateId::NewUp, &ctx)
            .unwrap();
        let actual_down = templates
            .render(TemplateGroup::Default, TemplateId::NewDown, &ctx)
            .unwrap();

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

    #[tokio::test]
    async fn named_templates() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();
        let templates_dir = config.templates_dir.unwrap();

        std::fs::create_dir_all(templates_dir.join("create_table")).unwrap();
        std::fs::write(templates_dir.join("create_table/new.up.sql"), CUSTOM_UP).unwrap();
        std::fs::write(templates_dir.join("create_table/new.down.sql"), CUSTOM_DOWN).unwrap();

        let templates = Templates::new(templates_dir).unwrap();

        let ctx = TemplateContext {
            id: MigrationId(123),
            name: String::from("custom"),
        };

        let group = TemplateGroup::Named("create_table".to_owned());

        let actual_up = templates.render(&group, TemplateId::NewUp, &ctx).unwrap();
        let actual_down = templates.render(&group, TemplateId::NewDown, &ctx).unwrap();

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
