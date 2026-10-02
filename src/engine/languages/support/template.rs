//! Byte-preserving masking for embedded Django, Jinja, and Vue template tags.

use std::borrow::Cow;

/// A template delimiter found in the original, unmodified source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TemplateTagKind {
    Statement,
    Interpolation,
    Comment,
}

/// Original source span and content for one template tag.
pub(crate) struct TemplateTag<'a> {
    pub(crate) kind: TemplateTagKind,
    pub(crate) content: &'a str,
    pub(crate) line: usize,
    pub(crate) closed: bool,
}

/// Replaces template bytes with spaces while retaining newlines and byte offsets.
pub(crate) struct TemplateMasker;

impl TemplateMasker {
    /// Masks all supported tags and optionally selected ampersands in one scan.
    pub(crate) fn mask<'a>(
        source: &'a str,
        mut on_tag: impl FnMut(TemplateTag<'a>),
        mask_ampersand: Option<fn(&str) -> bool>,
    ) -> Cow<'a, str> {
        if !source.contains("{%")
            && !source.contains("{{")
            && !source.contains("{#")
            && (mask_ampersand.is_none() || !source.contains('&'))
        {
            return Cow::Borrowed(source);
        }
        let bytes = source.as_bytes();
        let mut masked: Option<Vec<u8>> = None;
        let mut offset = 0;
        let mut line = 1;
        while offset < bytes.len() {
            if offset + 1 < bytes.len() && bytes[offset] == b'{' {
                let (kind, close) = match bytes[offset + 1] {
                    b'%' => (TemplateTagKind::Statement, "%}"),
                    b'{' => (TemplateTagKind::Interpolation, "}}"),
                    b'#' => (TemplateTagKind::Comment, "#}"),
                    _ => {
                        offset += 1;
                        continue;
                    }
                };
                let (end, closed) = source[offset + 2..]
                    .find(close)
                    .map_or((bytes.len(), false), |relative| {
                        (offset + 2 + relative + close.len(), true)
                    });
                let content = if closed {
                    &source[offset + 2..end - close.len()]
                } else {
                    &source[offset + 2..end]
                };
                on_tag(TemplateTag {
                    kind,
                    content,
                    line,
                    closed,
                });
                let output = masked.get_or_insert_with(|| bytes.to_vec());
                for (original, replacement) in bytes[offset..end]
                    .iter()
                    .zip(output[offset..end].iter_mut())
                {
                    if *original == b'\n' {
                        line += 1;
                    } else {
                        *replacement = b' ';
                    }
                }
                offset = end;
                continue;
            }
            if bytes[offset] == b'&'
                && mask_ampersand.is_some_and(|should_mask| should_mask(&source[offset..]))
            {
                masked.get_or_insert_with(|| bytes.to_vec())[offset] = b' ';
            }
            if bytes[offset] == b'\n' {
                line += 1;
            }
            offset += 1;
        }
        masked.map_or(Cow::Borrowed(source), |bytes| {
            Cow::Owned(String::from_utf8(bytes).expect("template masking retains valid UTF-8"))
        })
    }
}
