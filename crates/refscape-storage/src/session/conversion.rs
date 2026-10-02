//! Lossless v1/domain conversion. Wire defaults never enter runtime types.
use super::v1;
use refscape_model as model;

macro_rules! fields {
    ($name:ident { $($field:ident),* $(,)? }) => {
        impl From<v1::$name> for model::$name {
            fn from(value: v1::$name) -> Self { Self { $($field: value.$field.into()),* } }
        }
        impl From<&model::$name> for v1::$name {
            fn from(value: &model::$name) -> Self { Self { $($field: value.$field.clone().into()),* } }
        }
    };
}
fields!(Position { line, character });
fields!(SourceRange { start, end });
fields!(Point { x, y });

fields!(SourceContext { start_line, code });
fields!(SemanticToken {
    line,
    start,
    length,
    kind,
    modifiers
});
impl From<v1::ProjectOptions> for model::ProjectOpenOptions {
    fn from(value: v1::ProjectOptions) -> Self {
        Self {
            language: value.language.into(),
            compilation_database: value.compilation_database,
        }
    }
}
impl From<&model::ProjectOpenOptions> for v1::ProjectOptions {
    fn from(value: &model::ProjectOpenOptions) -> Self {
        Self {
            language: value.language.into(),
            compilation_database: value.compilation_database.clone(),
        }
    }
}
impl From<model::ProjectOpenOptions> for v1::ProjectOptions {
    fn from(value: model::ProjectOpenOptions) -> Self {
        (&value).into()
    }
}
fields!(Palette {
    background,
    surface,
    surface_alt,
    text,
    muted,
    accent,
    border,
    connection,
    syntax_keyword,
    syntax_string,
    syntax_type,
    syntax_function
});
fields!(Theme { name, palette });

macro_rules! owned_export {
    ($($name:ident),* $(,)?) => {$(
        impl From<model::$name> for v1::$name {
            fn from(value:model::$name)->Self { (&value).into() }
        }
    )*};
}
owned_export!(
    Position,
    SourceRange,
    Point,
    SourceContext,
    SemanticToken,
    Palette,
    Theme
);

macro_rules! enumeration {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        impl From<v1::$name> for model::$name {
            fn from(value: v1::$name) -> Self { match value { $(v1::$name::$variant => Self::$variant),* } }
        }
        impl From<model::$name> for v1::$name {
            fn from(value: model::$name) -> Self { match value { $(model::$name::$variant => Self::$variant),* } }
        }
    };
}
enumeration!(ProjectLanguage {
    Auto,
    Rust,
    Cpp,
    TypeScript,
    Python
});
enumeration!(ConnectionKind {
    Definition,
    TypeDefinition,
    Reference
});

impl From<v1::Symbol> for model::Symbol {
    fn from(value: v1::Symbol) -> Self {
        Self {
            id: value.id,
            name: value.name,
            kind: value.kind,
            path: value.path,
            range: value.range.into(),
            selection_range: value.selection_range.into(),
            children: value.children.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<&model::Symbol> for v1::Symbol {
    fn from(value: &model::Symbol) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            kind: value.kind.clone(),
            path: value.path.clone(),
            range: value.range.into(),
            selection_range: value.selection_range.into(),
            children: value.children.iter().map(Into::into).collect(),
        }
    }
}
impl From<v1::SourceDocument> for model::SourceDocument {
    fn from(value: v1::SourceDocument) -> Self {
        Self {
            symbol: value.symbol.into(),
            code: value.code,
            tokens: value.tokens.into_iter().map(Into::into).collect(),
            context: value.context.into_iter().map(Into::into).collect(),
            code_start: value.code_start.map(Into::into),
            folded: value.folded.into_iter().map(Into::into).collect(),
            expanded: value.expanded.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<&model::SourceDocument> for v1::SourceDocument {
    fn from(value: &model::SourceDocument) -> Self {
        Self {
            symbol: (&value.symbol).into(),
            code: value.code.clone(),
            tokens: value.tokens.iter().map(Into::into).collect(),
            context: value.context.iter().map(Into::into).collect(),
            code_start: value.code_start.map(Into::into),
            folded: value.folded.iter().map(Into::into).collect(),
            expanded: value.expanded.iter().map(Into::into).collect(),
        }
    }
}
impl TryFrom<v1::Viewport> for model::Viewport {
    type Error = model::RefscapeError;
    fn try_from(value: v1::Viewport) -> Result<Self, Self::Error> {
        let viewport = Self {
            offset: model::Point::from(value.offset).try_into()?,
            zoom: value.zoom,
        };
        viewport
            .validate()
            .map_err(|error| model::RefscapeError::new(model::ErrorKind::InvalidData, error))?;
        Ok(viewport)
    }
}
impl From<&model::Viewport> for v1::Viewport {
    fn from(value: &model::Viewport) -> Self {
        Self {
            offset: (&value.offset.point()).into(),
            zoom: value.zoom,
        }
    }
}
