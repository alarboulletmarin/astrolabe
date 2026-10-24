//! Back and forward through the pages shown, as in a browser (pure). In memory only.

/// A page and how far it was scrolled when left.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<T> {
    pub page: T,
    pub scroll: f32,
}

/// Number of entries kept: beyond that, the oldest go.
const KEPT: usize = 100;

/// A list and a position.
pub struct History<T> {
    list: Vec<Entry<T>>,
    at: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(first: T) -> Self {
        History { list: vec![Entry { page: first, scroll: 0.0 }], at: 0 }
    }

    pub fn current(&self) -> &T {
        &self.list[self.at].page
    }

    /// `page` is now shown. Returns false, and changes nothing, if it already was; otherwise
    /// what followed in the list is forgotten.
    pub fn visit(&mut self, page: T) -> bool {
        if *self.current() == page {
            return false;
        }
        self.list.truncate(self.at + 1);
        self.list.push(Entry { page, scroll: 0.0 });
        if self.list.len() > KEPT {
            self.list.remove(0);
        }
        self.at = self.list.len() - 1;
        true
    }

    /// `page` takes the current entry's place: a sideways move (from one activity to the next)
    /// that "back" should not walk through.
    pub fn replace(&mut self, page: T) {
        self.list[self.at] = Entry { page, scroll: 0.0 };
    }

    /// Remembers how far the current page is scrolled.
    pub fn mark(&mut self, scroll: f32) {
        self.list[self.at].scroll = scroll;
    }

    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.list.len()
    }

    pub fn back(&mut self) -> Option<Entry<T>> {
        self.can_back().then(|| {
            self.at -= 1;
            self.list[self.at].clone()
        })
    }

    pub fn forward(&mut self) -> Option<Entry<T>> {
        self.can_forward().then(|| {
            self.at += 1;
            self.list[self.at].clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_back_and_forward_like_a_browser() {
        let mut h = History::new('a');
        assert!(!h.can_back() && !h.can_forward() && h.back().is_none() && h.forward().is_none());
        assert!(h.visit('b') && h.visit('c'));
        // Showing again what is already there adds nothing.
        assert!(!h.visit('c'));
        assert_eq!(h.back().map(|e| e.page), Some('b'));
        assert_eq!(h.back().map(|e| e.page), Some('a'));
        assert!(h.back().is_none() && h.can_forward());
        assert_eq!(h.forward().map(|e| e.page), Some('b'));
        // Something else after going back: what followed is forgotten.
        h.visit('d');
        assert!(!h.can_forward() && h.forward().is_none());
        assert_eq!(h.back().map(|e| e.page), Some('b'));
        // The same page twice, apart: two entries.
        h.visit('a');
        h.visit('b');
        assert_eq!(h.back().map(|e| e.page), Some('a'));
    }

    #[test]
    fn remembers_how_far_each_page_was_scrolled() {
        let mut h = History::new('a');
        h.mark(40.0);
        h.visit('b');
        h.mark(7.0);
        assert_eq!(h.back(), Some(Entry { page: 'a', scroll: 40.0 }));
        assert_eq!(h.forward(), Some(Entry { page: 'b', scroll: 7.0 }));
        // A page visited anew starts at the top.
        h.back();
        h.visit('c');
        assert_eq!(h.back().map(|e| e.scroll), Some(40.0));
        assert_eq!(h.forward().map(|e| e.scroll), Some(0.0));
    }

    #[test]
    fn replace_is_a_sideways_move() {
        let mut h = History::new('j');
        h.visit('1');
        h.replace('2');
        h.replace('3');
        assert_eq!(h.current(), &'3');
        assert_eq!(h.back().map(|e| e.page), Some('j'));
        assert!(h.back().is_none());
        assert_eq!(h.forward().map(|e| e.page), Some('3'));
    }

    #[test]
    fn keeps_a_hundred() {
        let mut h = History::new(0);
        for i in 1..150 {
            h.visit(i);
        }
        assert_eq!(h.current(), &149);
        let mut back = 0;
        while h.back().is_some() {
            back += 1;
        }
        assert_eq!(back, KEPT - 1);
        assert_eq!(h.current(), &50);
    }
}
