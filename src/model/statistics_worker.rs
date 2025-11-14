use opencv::core::{self as cv, Mat, MatTraitConst, ModifyInplace, Scalar, Size};
use std::{
    collections::HashSet,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
};

use crate::util::cv_ext::MatExt;

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum StatisticsType {
    MinMax,
    Std,
    MSE,
    MAE,
    PSNRRMSE,
    SSIM,
    MSSSIM,
    FSIM,
}

impl StatisticsType {
    pub fn result_size(&self) -> usize {
        match self {
            StatisticsType::MinMax => 2,
            StatisticsType::Std => 1,
            StatisticsType::MSE => 1,
            StatisticsType::MAE => 1,
            StatisticsType::PSNRRMSE => 2,
            StatisticsType::SSIM => 1,
            StatisticsType::MSSSIM => 1,
            StatisticsType::FSIM => 1,
        }
    }
}

impl std::fmt::Display for StatisticsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            StatisticsType::MinMax => "Min/Max",
            StatisticsType::Std => "Std Dev",
            StatisticsType::MSE => "MSE",
            StatisticsType::MAE => "MAE",
            StatisticsType::PSNRRMSE => "PSNR/MSE",
            StatisticsType::SSIM => "SSIM",
            StatisticsType::MSSSIM => "MSSSIM",
            StatisticsType::FSIM => "FSIM",
        };
        write!(f, "{s}")
    }
}

pub struct StatisticsResult {
    pub stat_type: StatisticsType,
    pub value: Vec<f64>,
}

pub struct StatisticsUpdate {
    pub stat_type: StatisticsType,
    pub value: Vec<f64>,
    pub is_pending: bool,
}

struct SSIMMatData {
    img: Mat,
    img2: Mat,
    mu: Mat,
    mu2: Mat,
    sigma2: Mat,
}

fn ssim_blur(mat: &Mat) -> opencv::Result<Mat> {
    let mut blurred = Mat::default();
    let ksize = Size { width: 11, height: 11 };
    let sigma = 1.5;
    opencv::imgproc::gaussian_blur_def(mat, &mut blurred, ksize, sigma)?;
    Ok(blurred)
}

impl SSIMMatData {
    fn new(mat: Mat) -> opencv::Result<Self> {
        let img = mat;

        let mut img2 = Mat::default();
        cv::multiply_def(&img, &img, &mut img2)?;

        let mu = ssim_blur(&img)?;
        let mut mu2 = Mat::default();
        cv::multiply_def(&mu, &mu, &mut mu2)?;

        let mut sigma2 = ssim_blur(&img2)?;
        unsafe {
            sigma2.modify_inplace(|i, o| cv::subtract_def(i, &mu2, o))?;
        }

        Ok(Self {
            img,
            img2,
            mu,
            mu2,
            sigma2,
        })
    }
}

#[derive(Default)]
pub struct Statistics {
    pub rmse: f64,
    pub psnr: f64,
    pub ssim: f64,
    pub min: Vec<f64>,
    pub max: Vec<f64>,
}

pub struct StatisticsWorker {
    tx: Sender<StatisticsResult>,
    rx: Receiver<StatisticsResult>,

    processing: HashSet<StatisticsType>,
    pending: HashSet<StatisticsType>,
}

impl StatisticsWorker {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();

        Self {
            tx,
            rx,
            processing: HashSet::new(),
            pending: HashSet::new(),
        }
    }

    pub fn run_minmax(&mut self, mat: &Mat, scale: f64, rect: cv::Rect) {
        let _timer = crate::util::timer::ScopedTimer::new("Statistics::MinMaxWrapper");
        if mat.empty() {
            return;
        }
        let mat = mat.shallow_clone().unwrap();

        self.run(StatisticsType::MinMax, move || {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Statistics::MinMax");

            let mat_roi = cv::Mat::roi(&mat, rect)?;

            let mut result = Vec::new();

            // For single channel image
            if mat.channels() == 1 {
                let mut min_val = 0.0;
                let mut max_val = 0.0;

                cv::min_max_loc(&mat_roi, Some(&mut min_val), Some(&mut max_val), None, None, &cv::no_array())?;

                result.push(min_val * scale);
                result.push(max_val * scale);

                return Ok::<Vec<f64>, opencv::Error>(result);
            }

            // For multi-channel image
            let mut channels = cv::Vector::<cv::Mat>::new();
            // TODO: split copy data. split should be avoided for performance
            cv::split(&mat_roi, &mut channels)?;

            for i in 0..channels.len() {
                let ch = channels.get(i)?;

                let mut min_val = 0.0;
                let mut max_val = 0.0;

                cv::min_max_loc(&ch, Some(&mut min_val), Some(&mut max_val), None, None, &cv::no_array())?;

                result.push(min_val * scale);
                result.push(max_val * scale);
            }

            Ok::<Vec<f64>, opencv::Error>(result)
        });
    }

    pub fn run_psnr(&mut self, mat1: &Mat, mat2: &Mat, data_range: f64, scale: f64, rect: cv::Rect) {
        let _timer = crate::util::timer::ScopedTimer::new("Statistics::MinMaxWrapper");
        if mat1.empty() || mat2.empty() {
            return;
        }

        let mat1 = mat1.shallow_clone().unwrap();
        let mat2 = mat2.shallow_clone().unwrap();

        self.run(StatisticsType::PSNRRMSE, move || {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Statistics::PSNR");

            let mat1_roi = Mat::roi(&mat1, rect)?;
            let mat2_roi = Mat::roi(&mat2, rect)?;
            let psnr = cv::psnr(&mat1_roi, &mat2_roi, data_range)?;

            let rmse = scale / 10.0_f64.powf(psnr / 20.0);

            Ok::<Vec<f64>, opencv::Error>(vec![psnr, rmse])
        });
    }

    pub fn run_ssim(&mut self, mat1: &Mat, mat2: &Mat, rect: cv::Rect) {
        if mat1.empty() || mat2.empty() {
            return;
        }

        let mat1 = mat1.shallow_clone().unwrap();
        let mat2 = mat2.shallow_clone().unwrap();

        self.run(StatisticsType::SSIM, move || unsafe {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Statistics::SSIM");

            let mat1_roi = Mat::roi(&mat1, rect)?;
            let mat2_roi = Mat::roi(&mat2, rect)?;

            let lhs = SSIMMatData::new(mat1_roi.clone_pointee())?;
            let rhs = SSIMMatData::new(mat2_roi.clone_pointee())?;

            let c1: f64 = 0.0001; // c1 = 0.01^2 = 0.0001
            let c2: f64 = 0.0009; // c2 = 0.03^2 = 0.0009

            let mut img1_img2 = Mat::default();
            let mut mu1_mu2 = Mat::default();
            let mut t1 = Mat::default();
            let mut t2 = Mat::default();
            let mut t3 = Mat::default();
            let mut sigma12 = Mat::default();

            cv::multiply_def(&lhs.img, &rhs.img, &mut img1_img2)?;
            cv::multiply_def(&lhs.mu, &rhs.mu, &mut mu1_mu2)?;
            cv::subtract_def(&ssim_blur(&img1_img2)?, &mu1_mu2, &mut sigma12)?;

            // t3 = ((2*mu1_mu2 + C1).*(2*sigma12 + C2))
            cv::multiply_def(&mu1_mu2, &Scalar::all(2.0), &mut t1)?;
            t1.modify_inplace(|i, o| cv::add_def(i, &Scalar::all(c1), o))?;

            cv::multiply_def(&sigma12, &Scalar::all(2.0), &mut t2)?;
            t2.modify_inplace(|i, o| cv::add_def(i, &Scalar::all(c2), o))?;

            // t3 = t1 * t2
            cv::multiply_def(&t1, &t2, &mut t3)?;

            // t1 =((mu1_2 + mu2_2 + C1).*(sigma1_2 + sigma2_2 + C2))
            cv::add_def(&lhs.mu2, &rhs.mu2, &mut t1)?;
            t1.modify_inplace(|i, o| cv::add_def(i, &Scalar::all(c1), o))?;

            cv::add_def(&lhs.sigma2, &rhs.sigma2, &mut t2)?;
            t2.modify_inplace(|i, o| cv::add_def(i, &Scalar::all(c2), o))?;

            // t1 *= t2
            t1.modify_inplace(|i, o| cv::multiply_def(i, &t2, o))?;

            // quality map: t3 /= t1
            t3.modify_inplace(|i, o| cv::divide2_def(i, &t1, o))?;

            let ssim_c = cv::mean_def(&t3)?;

            Ok::<Vec<f64>, opencv::Error>(vec![
                (ssim_c[0] + ssim_c[1] + ssim_c[2] + ssim_c[3]) / img1_img2.channels() as f64,
            ])
        });
    }

    pub fn run<F, E>(&mut self, stat_type: StatisticsType, func: F)
    where
        F: FnOnce() -> Result<Vec<f64>, E> + Send + 'static,
        E: std::error::Error,
    {
        if self.processing.contains(&stat_type) {
            self.pending.insert(stat_type);
            return;
        } else {
            self.pending.remove(&stat_type);
        }

        self.processing.insert(stat_type.clone());

        let tx = self.tx.clone();
        thread::spawn(move || {
            match func() {
                Ok(val) => {
                    let _ = tx.send(StatisticsResult { stat_type, value: val });
                }
                Err(e) => {
                    eprintln!("StatisticsWorker: Error computing {}: {:?}", stat_type, e);
                    let result_size = stat_type.result_size();
                    let _ = tx.send(StatisticsResult {
                        stat_type,
                        value: vec![f64::NAN; result_size],
                    });
                }
            };
        });
    }

    pub fn invalidate(&mut self) -> Vec<StatisticsUpdate> {
        let mut invalidated = Vec::new();

        loop {
            match self.rx.try_recv() {
                Ok(msg) => {
                    self.processing.remove(&msg.stat_type);

                    // When statistics in calculated while another request is pending,
                    // mark it as pending again
                    let is_pending = self.pending.contains(&msg.stat_type);

                    invalidated.push(StatisticsUpdate {
                        stat_type: msg.stat_type,
                        value: msg.value,
                        is_pending,
                    });
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(_) => {
                    // Disconnected
                    eprintln!("StatisticsWorker channel disconnected");
                    break;
                }
            }
        }
        invalidated
    }
}
