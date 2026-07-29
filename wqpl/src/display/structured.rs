#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultRegionKind {
    Source,
    Axis(usize),
    Index(usize),
    Fence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultRegion {
    pub span: (usize, usize),
    pub kind: ResultRegionKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructuredPrintResult {
    pub text: String,
    pub regions: Vec<ResultRegion>,
}

impl StructuredPrintResult {
    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            regions: Vec::new(),
        }
    }

    pub(crate) fn region(text: impl Into<String>, kind: ResultRegionKind) -> Self {
        let text = text.into();
        let regions = if text.is_empty() {
            Vec::new()
        } else {
            vec![ResultRegion {
                span: (0, text.len()),
                kind,
            }]
        };
        Self { text, regions }
    }

    pub(crate) fn push_plain(&mut self, text: &str) {
        self.text.push_str(text);
    }

    pub(crate) fn append(&mut self, mut other: Self) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        self.regions
            .extend(other.regions.drain(..).map(|region| ResultRegion {
                span: (region.span.0 + offset, region.span.1 + offset),
                kind: region.kind,
            }));
    }

    pub(crate) fn joined(items: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        let mut result = Self::default();
        for (index, item) in items.into_iter().enumerate() {
            if index > 0 {
                result.push_plain(separator);
            }
            result.append(item);
        }
        result
    }

    pub(crate) fn render_with(
        &self,
        mut render_region: impl FnMut(ResultRegionKind, &str) -> String,
    ) -> String {
        let mut output = String::with_capacity(self.text.len());
        let mut cursor = 0;
        for region in &self.regions {
            debug_assert!(region.span.0 >= cursor);
            debug_assert!(region.span.1 >= region.span.0);
            debug_assert!(region.span.1 <= self.text.len());
            output.push_str(&self.text[cursor..region.span.0]);
            output.push_str(&render_region(
                region.kind,
                &self.text[region.span.0..region.span.1],
            ));
            cursor = region.span.1;
        }
        output.push_str(&self.text[cursor..]);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appending_structured_text_rebases_regions() {
        let mut result = StructuredPrintResult::plain("head ");
        result.append(StructuredPrintResult::region(
            "42",
            ResultRegionKind::Source,
        ));
        result.push_plain(" ");
        result.append(StructuredPrintResult::region("|", ResultRegionKind::Fence));

        assert_eq!(result.text, "head 42 |");
        assert_eq!(
            result.regions,
            [
                ResultRegion {
                    span: (5, 7),
                    kind: ResultRegionKind::Source,
                },
                ResultRegion {
                    span: (8, 9),
                    kind: ResultRegionKind::Fence,
                },
            ]
        );
    }
}
