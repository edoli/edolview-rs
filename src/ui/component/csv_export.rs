pub struct CsvExportPayload {
    pub title: &'static str,
    pub suggested_file_name: &'static str,
    pub csv_text: String,
}

impl CsvExportPayload {
    pub fn new(title: &'static str, suggested_file_name: &'static str, csv_text: String) -> Self {
        Self {
            title,
            suggested_file_name,
            csv_text,
        }
    }
}

pub enum CsvExportAction {
    Copy(CsvExportPayload),
    Save(CsvExportPayload),
}
