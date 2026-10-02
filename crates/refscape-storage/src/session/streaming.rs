//! Borrowed serializers: encoding shares all body, symbol and token storage.
use super::{SESSION_VERSION, v1};
use refscape_application::ApplicationSnapshot;
use refscape_model::{
    CardSource, CodeCard, Connection, Region, SemanticToken, SourceContext, Symbol,
};
use serde::{
    Serialize, Serializer,
    ser::{SerializeSeq, SerializeStruct},
};
pub(crate) struct SessionView<'a>(pub &'a ApplicationSnapshot);
impl Serialize for SessionView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct("Session", 8)?;
        s.serialize_field("version", &SESSION_VERSION)?;
        s.serialize_field("project_root", &v.project_root)?;
        s.serialize_field(
            "project_options",
            &v1::ProjectOptions::from(&v.project_options),
        )?;
        s.serialize_field("cards", &Cards(&v.cards))?;
        s.serialize_field("connections", &Connections(&v.connections))?;
        s.serialize_field("regions", &Regions(&v.regions))?;
        s.serialize_field("viewport", &v1::Viewport::from(&v.viewport))?;
        s.serialize_field("theme", &v1::Theme::from(&v.theme))?;
        s.end()
    }
}
macro_rules! sequence {
    ($name:ident,$item:ty,$wrapper:ident) => {
        struct $name<'a>(&'a [$item]);
        impl Serialize for $name<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut s = serializer.serialize_seq(Some(self.0.len()))?;
                for item in self.0 {
                    s.serialize_element(&$wrapper(item))?;
                }
                s.end()
            }
        }
    };
}
sequence!(Cards, CodeCard, CardView);
sequence!(Connections, Connection, ConnectionView);
sequence!(Regions, Region, RegionView);
sequence!(Symbols, Symbol, SymbolView);
sequence!(Tokens, SemanticToken, TokenView);
sequence!(Contexts, SourceContext, ContextView);
struct CardView<'a>(&'a CodeCard);
impl Serialize for CardView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct("CodeCard", 5)?;
        s.serialize_field("id", v.id.as_str())?;
        s.serialize_field("source", &SourceView(&v.source))?;
        s.serialize_field("position", &v1::Point::from(&v.position.point()))?;
        s.serialize_field("width", &v.width)?;
        s.serialize_field("height", &v.height)?;
        s.end()
    }
}
struct SourceView<'a>(&'a CardSource);
impl Serialize for SourceView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct(
            "SourceDocument",
            7 + usize::from(v.document_fingerprint.is_some()),
        )?;
        s.serialize_field("symbol", &SymbolView(&v.symbol))?;
        s.serialize_field("code", v.code.as_ref())?;
        s.serialize_field("tokens", &Tokens(&v.tokens))?;
        s.serialize_field("context", &Contexts(&v.export_context()))?;
        s.serialize_field("code_start", &v.code_start.map(v1::Position::from))?;
        s.serialize_field("folded", &GapViews(v.export_folded()))?;
        s.serialize_field("expanded", &GapViews(v.export_expanded()))?;
        if let Some(fingerprint) = &v.document_fingerprint {
            s.serialize_field(
                "document_fingerprint",
                &v1::DocumentFingerprint::from(fingerprint),
            )?;
        }
        s.end()
    }
}
struct GapViews<'a>(Vec<&'a SourceContext>);
impl Serialize for GapViews<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_seq(Some(self.0.len()))?;
        for gap in &self.0 {
            s.serialize_element(&ContextView(gap))?;
        }
        s.end()
    }
}
struct ContextView<'a>(&'a SourceContext);
impl Serialize for ContextView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("SourceContext", 2)?;
        s.serialize_field("start_line", &self.0.start_line)?;
        s.serialize_field("code", &self.0.code)?;
        s.end()
    }
}
struct SymbolView<'a>(&'a Symbol);
impl Serialize for SymbolView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct("Symbol", 7)?;
        s.serialize_field("id", &v.id)?;
        s.serialize_field("name", &v.name)?;
        s.serialize_field("kind", &v.kind)?;
        s.serialize_field("path", &v.path)?;
        s.serialize_field("range", &v1::SourceRange::from(&v.range))?;
        s.serialize_field(
            "selection_range",
            &v1::SourceRange::from(&v.selection_range),
        )?;
        s.serialize_field("children", &Symbols(&v.children))?;
        s.end()
    }
}
struct TokenView<'a>(&'a SemanticToken);
impl Serialize for TokenView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct("SemanticToken", 5)?;
        s.serialize_field("line", &v.line)?;
        s.serialize_field("start", &v.start)?;
        s.serialize_field("length", &v.length)?;
        s.serialize_field("kind", &v.kind)?;
        s.serialize_field("modifiers", &v.modifiers)?;
        s.end()
    }
}
struct ConnectionView<'a>(&'a Connection);
impl Serialize for ConnectionView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct("Connection", 5)?;
        s.serialize_field("id", v.id.as_str())?;
        s.serialize_field("from", v.from.as_str())?;
        s.serialize_field("to", v.to.as_str())?;
        s.serialize_field("kind", &v1::ConnectionKind::from(v.kind))?;
        s.serialize_field("source", &v1::Position::from(v.source))?;
        s.end()
    }
}
struct RegionView<'a>(&'a Region);
impl Serialize for RegionView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        let mut s = serializer.serialize_struct("Region", 4)?;
        s.serialize_field("id", &v.id)?;
        s.serialize_field("label", &v.label)?;
        s.serialize_field("path", &v.path)?;
        s.serialize_field("card_ids", &CardIds(&v.card_ids))?;
        s.end()
    }
}
struct CardIds<'a>(&'a [refscape_model::CardId]);
impl Serialize for CardIds<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_seq(Some(self.0.len()))?;
        for id in self.0 {
            s.serialize_element(id.as_str())?;
        }
        s.end()
    }
}
