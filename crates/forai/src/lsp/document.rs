use std::collections::HashMap;

use lsp_types::Uri;

use super::line_index::LineIndex;

pub struct Document {
    pub text: String,
    pub line_index: LineIndex,
    pub version: i32,
}

impl Document {
    pub fn new(text: String, version: i32) -> Self {
        let line_index = LineIndex::new(&text);
        Document {
            text,
            line_index,
            version,
        }
    }
}

pub struct DocumentStore {
    docs: HashMap<Uri, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        DocumentStore {
            docs: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: Uri, text: String, version: i32) {
        self.docs.insert(uri, Document::new(text, version));
    }

    pub fn change(&mut self, uri: &Uri, text: String, version: i32) {
        self.docs.insert(uri.clone(), Document::new(text, version));
    }

    pub fn close(&mut self, uri: &Uri) {
        self.docs.remove(uri);
    }

    pub fn get(&self, uri: &Uri) -> Option<&Document> {
        self.docs.get(uri)
    }
}
