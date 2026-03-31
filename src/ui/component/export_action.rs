pub struct CopyExport {
    pub title: &'static str,
    pub text: String,
}

pub struct SaveExport {
    pub title: &'static str,
    pub suggested_file_name: &'static str,
    pub text: String,
}

pub enum ExportAction {
    Copy(CopyExport),
    Save(SaveExport),
}
