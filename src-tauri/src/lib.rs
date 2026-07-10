mod commands;
mod config;
mod credentials;
mod db;
mod elevated_sftp;
mod logs;
mod sftp;
mod ssh;
mod state;
mod transfer;
mod tunnel;

use state::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let paths = config::AppPaths::resolve(app.handle())?;
            std::fs::create_dir_all(&paths.data_dir)?;
            std::fs::create_dir_all(&paths.log_dir)?;
            db::initialize(&paths.database_path)?;
            logs::append_event(&paths.log_dir, "info", "app", "应用启动")?;
            app.manage(AppState::new(paths));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::initialize_database,
            commands::get_app_status,
            commands::list_groups,
            commands::list_servers,
            commands::get_host_key_status,
            commands::trust_host_key,
            commands::clear_trusted_host_key,
            commands::get_log_directory,
            commands::open_log_directory,
            commands::create_server,
            commands::update_server,
            commands::duplicate_server,
            commands::delete_server,
            commands::list_tunnel_rules,
            commands::save_tunnel_rule,
            commands::start_tunnel,
            commands::stop_tunnel,
            commands::list_active_tunnels,
            commands::delete_tunnel_rule,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::test_connection,
            commands::open_terminal,
            commands::terminal_write,
            commands::terminal_resize,
            commands::close_terminal,
            commands::sftp_read_dir,
            commands::sftp_create_dir,
            commands::sftp_create_file,
            commands::sftp_delete,
            commands::sftp_rename,
            commands::sftp_upload_file,
            commands::sftp_download_file,
            commands::sftp_preview_file,
            commands::sftp_read_text_file,
            commands::sftp_write_text_file,
            commands::sftp_set_permissions,
            commands::sftp_checksum,
            commands::sftp_compress_paths,
            commands::sftp_open_remote_file,
            commands::pick_upload_file,
            commands::pick_upload_directory,
            commands::pick_download_path,
            commands::pick_download_directory,
            commands::get_app_info,
            commands::read_logs,
            commands::transfer_start,
            commands::transfer_cancel,
            commands::transfer_retry,
            commands::get_server_stats
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ALAX SSH Manager");
}
