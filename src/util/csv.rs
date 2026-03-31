use std::fmt::{Display, Write as _};

use crate::util::series::SeriesLike;

fn visible_series_indices(series_len: usize, mask: &[bool]) -> Vec<usize> {
    (0..series_len).filter(|&i| mask.get(i).copied().unwrap_or(false)).collect()
}

pub fn build_indexed_series_csv<T: Display + Copy, S: SeriesLike<T>>(
    index_label: &str,
    absolute_index_label: Option<&str>,
    index_offset: Option<i32>,
    value_label_prefix: &str,
    series: S,
    mask: &[bool],
) -> String {
    let visible_series = visible_series_indices(series.len(), mask);
    let len = series.first_len();

    let mut csv = String::new();
    let _ = write!(csv, "{index_label}");
    if let Some(label) = absolute_index_label {
        let _ = write!(csv, ",{label}");
    }
    for &i in &visible_series {
        let _ = write!(csv, ",{value_label_prefix}{i}");
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
