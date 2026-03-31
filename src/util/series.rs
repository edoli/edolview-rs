use std::fmt::{Display, Write as _};

pub struct SeriesRef<'a, T> {
    slices: &'a [&'a [T]],
}

pub struct SeriesBuffer<T> {
    data: Vec<Vec<T>>,
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

impl<T> SeriesBuffer<T> {
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
    pub fn scaled(self, scale: f64) -> SeriesBuffer<f64> {
        SeriesBuffer::new(
            self.iter()
                .map(|values| values.iter().map(|value| *value * scale).collect())
                .collect(),
        )
    }
}

pub trait SeriesSource<T> {
    fn len(&self) -> usize;
    fn get(&self, index: usize) -> Option<&[T]>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn first_len(&self) -> usize {
        self.get(0).map_or(0, |values| values.len())
    }
}

impl<'a, T> SeriesSource<T> for SeriesRef<'a, T> {
    fn len(&self) -> usize {
        self.slices.len()
    }

    fn get(&self, index: usize) -> Option<&[T]> {
        self.slices.get(index).copied()
    }
}

impl<T> SeriesSource<T> for SeriesBuffer<T> {
    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, index: usize) -> Option<&[T]> {
        self.get(index)
    }
}

fn visible_series_indices(series_len: usize, mask: &[bool]) -> Vec<usize> {
    (0..series_len).filter(|&i| mask.get(i).copied().unwrap_or(false)).collect()
}

pub fn channel_label(index: usize, total_channels: usize) -> String {
    if total_channels == 1 {
        return "L".to_string();
    }

    match index {
        0 => "R".to_string(),
        1 => "G".to_string(),
        2 => "B".to_string(),
        3 => "A".to_string(),
        _ => format!("C{index}"),
    }
}

pub fn build_indexed_csv<T: Display + Copy>(
    index_label: &str,
    absolute_index_label: Option<&str>,
    index_offset: Option<i32>,
    series: &impl SeriesSource<T>,
    mask: &[bool],
) -> String {
    let visible_series = visible_series_indices(series.len(), mask);
    let total_channels = series.len();
    let len = series.first_len();

    let mut csv = String::new();
    let _ = write!(csv, "{index_label}");
    if let Some(label) = absolute_index_label {
        let _ = write!(csv, ",{label}");
    }
    for &i in &visible_series {
        let _ = write!(csv, ",{}", channel_label(i, total_channels));
    }
    csv.push('\n');

    for idx in 0..len {
        let _ = write!(csv, "{idx}");
        if let Some(offset) = index_offset {
            let _ = write!(csv, ",{}", offset + idx as i32);
        }
        for &series_idx in &visible_series {
            let value = series.get(series_idx).and_then(|values| values.get(idx)).copied();
            if let Some(value) = value {
                let _ = write!(csv, ",{value}");
            } else {
                csv.push(',');
            }
        }
        csv.push('\n');
    }

    csv
}
