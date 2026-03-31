pub struct SeriesRef<'a, T> {
    slices: &'a [&'a [T]],
}

pub struct OwnedSeries<T> {
    data: Vec<Vec<T>>,
}

pub trait SeriesLike<T> {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn first_len(&self) -> usize;
    fn get(&self, index: usize) -> Option<&[T]>;
}

impl<'a, T> Copy for SeriesRef<'a, T> {}

impl<'a, T> Clone for SeriesRef<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> SeriesRef<'a, T> {
    pub const fn new(slices: &'a [&'a [T]]) -> Self {
        Self { slices }
    }

    pub fn len(self) -> usize {
        self.slices.len()
    }

    pub fn is_empty(self) -> bool {
        self.slices.is_empty()
    }

    pub fn first_len(self) -> usize {
        self.slices.first().map_or(0, |values| values.len())
    }

    pub fn get(self, index: usize) -> Option<&'a [T]> {
        self.slices.get(index).copied()
    }

    pub fn iter(self) -> impl Iterator<Item = &'a [T]> + 'a {
        self.slices.iter().copied()
    }
}

impl<T> OwnedSeries<T> {
    pub fn new(data: Vec<Vec<T>>) -> Self {
        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn first_len(&self) -> usize {
        self.data.first().map_or(0, |values| values.len())
    }

    pub fn get(&self, index: usize) -> Option<&[T]> {
        self.data.get(index).map(|values| values.as_slice())
    }

    pub fn iter(&self) -> impl Iterator<Item = &[T]> {
        self.data.iter().map(|values| values.as_slice())
    }
}

impl<'a> SeriesRef<'a, f64> {
    pub fn scaled(self, scale: f64) -> OwnedSeries<f64> {
        OwnedSeries::new(
            self.iter()
                .map(|values| values.iter().map(|value| *value * scale).collect())
                .collect(),
        )
    }
}

impl<'a, T> SeriesLike<T> for SeriesRef<'a, T> {
    fn len(&self) -> usize {
        self.slices.len()
    }

    fn first_len(&self) -> usize {
        self.slices.first().map_or(0, |values| values.len())
    }

    fn get(&self, index: usize) -> Option<&[T]> {
        self.slices.get(index).copied()
    }
}

impl<T> SeriesLike<T> for OwnedSeries<T> {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn first_len(&self) -> usize {
        self.data.first().map_or(0, |values| values.len())
    }

    fn get(&self, index: usize) -> Option<&[T]> {
        self.data.get(index).map(|values| values.as_slice())
    }
}
