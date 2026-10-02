#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub background: String,
    pub surface: String,
    pub surface_alt: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub border: String,
    pub connection: String,
    pub syntax_keyword: String,
    pub syntax_string: String,
    pub syntax_type: String,
    pub syntax_function: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self::from_colors(
            "Dark",
            [
                "#111820", "#1A2430", "#243342", "#E5EDF5", "#94A9BC", "#60D7BD", "#344858",
                "#62B8C5", "#CB9BF4", "#A4CD85", "#EAC071", "#79C5E9",
            ],
        )
    }

    pub fn light() -> Self {
        Self::from_colors(
            "Light",
            [
                "#EDF2F5", "#FFFFFF", "#F2F6F8", "#233443", "#657787", "#087F73", "#CAD6DD",
                "#438C9F", "#8554AB", "#397A31", "#99682B", "#246D9B",
            ],
        )
    }

    fn from_colors(name: &str, colors: [&str; 12]) -> Self {
        let [
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
            syntax_function,
        ] = colors.map(str::to_owned);
        Self {
            name: name.into(),
            palette: Palette {
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
                syntax_function,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let p = &self.palette;
        for color in [
            &p.background,
            &p.surface,
            &p.surface_alt,
            &p.text,
            &p.muted,
            &p.accent,
            &p.border,
            &p.connection,
            &p.syntax_keyword,
            &p.syntax_string,
            &p.syntax_type,
            &p.syntax_function,
        ] {
            if color.len() != 7
                || !color.starts_with('#')
                || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err(format!("Invalid theme color {color}; expected #RRGGBB"));
            }
        }
        if self.name.trim().is_empty() {
            return Err("Theme name must be nonempty".into());
        }
        Ok(())
    }
}
