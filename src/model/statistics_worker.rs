use opencv::{
    boxed_ref::BoxedRef,
    core::{Mat, MatTraitConst},
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

pub struct StatisticsWorker {
    tx: Sender<StatisticsResult>,
    rx: Receiver<StatisticsResult>,

    processing: HashSet<StatisticsType>,
}

impl StatisticsWorker {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();

        Self {
            tx,
            rx,
            processing: HashSet::new(),
        }
    }

    pub fn run_psnr(&mut self, img1: BoxedRef<'_, Mat>, img2: BoxedRef<'_, Mat>, data_range: f64) {
        if img1.empty() || img2.empty() {
            return;
        }

        let img1 = img1.clone_pointee();
        let img2 = img2.clone_pointee();

        self.run(StatisticsType::PSNR, move || {
            opencv::core::psnr(&img1, &img2, data_range).map_err(|_| ())
        });
    }

    pub fn run<F>(&mut self, stat_type: StatisticsType, func: F)
    where
        F: FnOnce() -> Result<f64, ()> + Send + 'static,
    {
        if self.processing.contains(&stat_type) {
            println!("StatisticsWorker: {} is already being processed", stat_type);
            return;
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

    pub fn invalidate(&mut self) -> Vec<StatisticsType> {
        let mut invalidated = Vec::new();

        loop {
            match self.rx.try_recv() {
                Ok(msg) => {
                    println!("StatisticsWorker: {} = {}", msg.stat_type, msg.value);
                    self.processing.remove(&msg.stat_type);
                    invalidated.push(msg.stat_type);
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
