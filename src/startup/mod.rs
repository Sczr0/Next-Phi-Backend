/// 启动数据加载器（difficulty.csv）
pub mod chart_loader;
/// 歌曲与别名加载器（info.csv / nicklist.yaml）
pub mod song_loader;
/// 启动检查工具模块
pub mod startup;

pub use startup::run_startup_checks;
