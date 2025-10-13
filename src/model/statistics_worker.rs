use opencv::{
    boxed_ref::BoxedRef,
    core::{self as cv, Mat, MatTraitConst, ModifyInplace, Scalar, Size},
};
use std::{
    collections::HashSet,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
};

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum StatisticsType {
    MinMax,
    Std,
    MSE,
    MAE,
    PSNR,
    SSIM,
    MSSSIM,
    FSIM,
}

impl std::fmt::Display for StatisticsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            StatisticsType::MinMax => "Min/Max",
            StatisticsType::Std => "Std Dev",
            StatisticsType::MSE => "MSE",
            StatisticsType::MAE => "MAE",
            StatisticsType::PSNR => "PSNR",
            StatisticsType::SSIM => "SSIM",
            StatisticsType::MSSSIM => "MSSSIM",
            StatisticsType::FSIM => "FSIM",
        };
        write!(f, "{s}")
    }
}

pub struct StatisticsResult {
    pub stat_type: StatisticsType,
    pub value: f64,
}

pub struct StatisticsUpdate {
    pub stat_type: StatisticsType,
    pub value: f64,
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
    fn new(mat: &Mat) -> opencv::Result<Self> {
        let img = mat.clone();

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
    pub psnr: f64,
    pub ssim: f64,
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

    pub fn run_psnr(&mut self, img1: &BoxedRef<'_, Mat>, img2: &BoxedRef<'_, Mat>, data_range: f64) {
        if img1.empty() || img2.empty() {
            return;
        }

        let img1 = img1.clone_pointee();
        let img2 = img2.clone_pointee();

        self.run(StatisticsType::PSNR, move || {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Statistics::PSNR");

            cv::psnr(&img1, &img2, data_range)
        });
    }

    pub fn run_ssim(&mut self, img1: &BoxedRef<'_, Mat>, img2: &BoxedRef<'_, Mat>) {
        if img1.empty() || img2.empty() {
            return;
        }

        let img1 = img1.clone_pointee();
        let img2 = img2.clone_pointee();

        self.run(StatisticsType::SSIM, move || unsafe {
            #[cfg(debug_assertions)]
            let _timer = crate::util::timer::ScopedTimer::new("Statistics::SSIM");

            let lhs = SSIMMatData::new(&img1)?;
            let rhs = SSIMMatData::new(&img2)?;

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

            Ok::<f64, opencv::Error>((ssim_c[0] + ssim_c[1] + ssim_c[2] + ssim_c[3]) / img1_img2.channels() as f64)
        });
    }

    pub fn run<F, E>(&mut self, stat_type: StatisticsType, func: F)
    where
        F: FnOnce() -> Result<f64, E> + Send + 'static,
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
                    let _ = tx.send(StatisticsResult {
                        stat_type,
                        value: f64::NAN,
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
