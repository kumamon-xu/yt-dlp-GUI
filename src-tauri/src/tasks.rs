//! 任务管理：并发队列、持久化、杀进程树、进度事件

use crate::command::{build_args, NewTask};
use crate::parser::{friendly_error, parse_progress, FILE_PREFIX};
use crate::{find_engine, find_ffmpeg, js_runtime_arg, kill_process_tree, no_window_cmd, AppState};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskPayload {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub status: String,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub eta: f64,
    pub file_path: Option<String>,
    pub error: Option<String>,
    pub request: NewTask,
}

#[derive(Serialize, Deserialize, Clone)]
struct TaskSnapshot {
    payload: TaskPayload,
}

struct TaskInner {
    payload: Mutex<TaskPayload>,
    child: Mutex<Option<Child>>,
    pid: AtomicU32,
    stderr_tail: Mutex<VecDeque<String>>,
    canceled: AtomicBool,
    args: Mutex<Vec<String>>,
}

pub struct TaskManager {
    tasks: Mutex<HashMap<String, Arc<TaskInner>>>,
    order: Mutex<Vec<String>>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
        }
    }
}

fn running_status(s: &str) -> bool {
    s == "downloading" || s == "postprocess"
}

/// `order` 新任务在前；取最后一个 queued，先入先跑。
fn oldest_queued<'a>(order: &'a [String], is_queued: impl Fn(&str) -> bool) -> Option<&'a str> {
    order.iter().rev().map(|s| s.as_str()).find(|id| is_queued(id))
}

fn persist(app: &AppHandle) {
    let mgr = app.state::<TaskManager>();
    let mut snaps = Vec::new();
    let order = mgr.order.lock().unwrap().clone();
    let map = mgr.tasks.lock().unwrap();
    for id in order {
        if let Some(t) = map.get(&id) {
            snaps.push(TaskSnapshot {
                payload: t.payload.lock().unwrap().clone(),
            });
        }
    }
    drop(map);
    let path = crate::config::queue_path(app);
    if let Ok(json) = serde_json::to_string_pretty(&snaps) {
        let _ = std::fs::write(path, json);
    }
}

pub fn restore_queue(app: &AppHandle) {
    let path = crate::config::queue_path(app);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(snaps): Result<Vec<TaskSnapshot>, _> = serde_json::from_str(&text) else {
        return;
    };
    let mgr = app.state::<TaskManager>();
    let mut map = mgr.tasks.lock().unwrap();
    let mut order = mgr.order.lock().unwrap();
    for snap in snaps {
        let mut p = snap.payload;
        if running_status(&p.status) {
            p.status = "paused".into();
            p.speed = 0.0;
        }
        order.push(p.id.clone());
        map.insert(
            p.id.clone(),
            Arc::new(TaskInner {
                payload: Mutex::new(p),
                child: Mutex::new(None),
                pid: AtomicU32::new(0),
                stderr_tail: Mutex::new(VecDeque::new()),
                canceled: AtomicBool::new(false),
                args: Mutex::new(Vec::new()),
            }),
        );
    }
}

fn emit_payload(app: &AppHandle, inner: &TaskInner) {
    let p = inner.payload.lock().unwrap().clone();
    let _ = app.emit("task_updated", &p);
}

async fn read_stdout(stdout: tokio::process::ChildStdout, app: AppHandle, inner: Arc<TaskInner>) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if inner.canceled.load(Ordering::SeqCst) {
            break;
        }
        if let Some((status, dl, total, speed, eta, title)) = parse_progress(&line) {
            let new_status = if status == "finished" {
                "postprocess"
            } else {
                "downloading"
            };
            {
                let mut p = inner.payload.lock().unwrap();
                if p.status != "canceled" && p.status != "paused" {
                    p.status = new_status.into();
                }
                p.downloaded = dl;
                p.total = total;
                p.speed = speed;
                p.eta = eta;
                if !title.is_empty() {
                    p.title = Some(title);
                }
            }
            emit_payload(&app, &inner);
        } else if let Some(path) = line.strip_prefix(FILE_PREFIX) {
            inner.payload.lock().unwrap().file_path = Some(path.trim().to_string());
            emit_payload(&app, &inner);
        } else if !line.trim().is_empty() {
            let id = inner.payload.lock().unwrap().id.clone();
            let _ = app.emit("task_log", serde_json::json!({ "id": id, "line": line }));
        }
    }
}

async fn read_stderr(stderr: tokio::process::ChildStderr, app: AppHandle, inner: Arc<TaskInner>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        {
            let mut tail = inner.stderr_tail.lock().unwrap();
            tail.push_back(line.clone());
            if tail.len() > 10 {
                tail.pop_front();
            }
        }
        if !line.trim().is_empty() {
            let id = inner.payload.lock().unwrap().id.clone();
            let _ = app.emit(
                "task_log",
                serde_json::json!({ "id": id, "line": format!("[stderr] {line}") }),
            );
        }
    }
}

fn finalize(app: &AppHandle, inner: &Arc<TaskInner>, success: bool) {
    let canceled = inner.canceled.load(Ordering::SeqCst);
    let paused = inner.payload.lock().unwrap().status == "paused";
    {
        let mut p = inner.payload.lock().unwrap();
        p.speed = 0.0;
        if canceled && p.status != "paused" {
            p.status = "canceled".into();
        } else if paused {
        } else if success {
            p.status = "done".into();
        } else {
            p.status = "failed".into();
            let tail: String = inner
                .stderr_tail
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            p.error = Some(friendly_error(&tail));
        }
    }
    emit_payload(app, inner);
}

fn settings_snapshot(app: &AppHandle) -> crate::GlobalSettings {
    app.state::<AppState>()
        .settings
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default()
}

fn apply_settings(task: &mut NewTask, s: &crate::GlobalSettings) {
    if task.out_dir.as_ref().map(|x| x.is_empty()).unwrap_or(true) && !s.out_dir.is_empty() {
        task.out_dir = Some(s.out_dir.clone());
    }
    if task.out_template.as_ref().map(|x| x.is_empty()).unwrap_or(true) && !s.out_template.is_empty() {
        task.out_template = Some(s.out_template.clone());
    }
    if task.concurrent_fragments.is_none() {
        task.concurrent_fragments = Some(s.concurrent_fragments);
    }
    if task.limit_rate.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
        task.limit_rate = s.limit_rate.clone();
    }
    if task.cookies_file.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
        task.cookies_file = s.cookies_file.clone();
    }
    if task.cookies_browser.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
        task.cookies_browser = s.cookies_browser.clone();
    }
    if task.proxy.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
        task.proxy = s.proxy.clone();
    }
    if task.merge_format.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
        task.merge_format = Some(s.merge_format.clone());
    }
}

fn spawn_download(app: AppHandle, inner: Arc<TaskInner>) -> Result<(), String> {
    let settings = settings_snapshot(&app);
    let engine = find_engine(settings.engine_path.as_deref()).ok_or_else(|| "未找到 yt-dlp 引擎".to_string())?;

    let mut request = inner.payload.lock().unwrap().request.clone();
    apply_settings(&mut request, &settings);
    let mut cfg = request.to_config();
    cfg.ffmpeg_location = find_ffmpeg(settings.ffmpeg_path.as_deref()).map(|p| p.to_string_lossy().into());
    cfg.js_runtime = js_runtime_arg();
    if inner.payload.lock().unwrap().status == "paused" || request.resume.unwrap_or(false) {
        cfg.resume = true;
    }
    let args = build_args(&cfg);
    *inner.args.lock().unwrap() = args.clone();

    let dir = if cfg.out_dir.is_empty() {
        crate::command::default_out_dir()
    } else {
        cfg.out_dir.clone()
    };
    let _ = std::fs::create_dir_all(&dir);

    let mut cmd = no_window_cmd(&engine);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("stdout 管道失败")?;
    let stderr = child.stderr.take().ok_or("stderr 管道失败")?;
    if let Some(pid) = child.id() {
        inner.pid.store(pid, Ordering::SeqCst);
    }
    *inner.child.lock().unwrap() = Some(child);
    inner.canceled.store(false, Ordering::SeqCst);
    {
        let mut p = inner.payload.lock().unwrap();
        p.status = "downloading".into();
        p.error = None;
        p.request.resume = Some(cfg.resume);
    }
    emit_payload(&app, &inner);

    let (a1, a2, a3) = (app.clone(), app.clone(), app.clone());
    let (i1, i2, i3) = (Arc::clone(&inner), Arc::clone(&inner), Arc::clone(&inner));
    tokio::spawn(async move { read_stdout(stdout, a1, i1).await });
    tokio::spawn(async move { read_stderr(stderr, a2, i2).await });
    tokio::spawn(async move {
        let child = i3.child.lock().unwrap().take();
        let success = match child {
            Some(mut c) => c.wait().await.map(|s| s.success()).unwrap_or(false),
            None => false,
        };
        finalize(&a3, &i3, success);
        i3.pid.store(0, Ordering::SeqCst);
        persist(&a3);
        pump_queue(a3);
    });
    Ok(())
}

fn max_concurrent(app: &AppHandle) -> u32 {
    app.state::<AppState>()
        .settings
        .lock()
        .ok()
        .map(|s| s.max_concurrent_tasks.max(1))
        .unwrap_or(2)
}

fn running_count(mgr: &TaskManager) -> u32 {
    let map = mgr.tasks.lock().unwrap();
    let mut n = 0u32;
    for t in map.values() {
        let st = t.payload.lock().unwrap().status.clone();
        if running_status(&st) {
            n += 1;
        }
    }
    n
}

fn pump_queue(app: AppHandle) {
    let cap = max_concurrent(&app);
    loop {
        let mgr = app.state::<TaskManager>();
        if running_count(mgr.inner()) >= cap {
            return;
        }
        let next = {
            let order = mgr.order.lock().unwrap().clone();
            let map = mgr.tasks.lock().unwrap();
            let mut found = None;
            if let Some(id) = oldest_queued(&order, |id| {
                map.get(id)
                    .map(|t| t.payload.lock().unwrap().status == "queued")
                    .unwrap_or(false)
            }) {
                if let Some(t) = map.get(id) {
                    found = Some(Arc::clone(t));
                }
            }
            found
        };
        drop(mgr);
        let Some(inner) = next else { return };
        if let Err(e) = spawn_download(app.clone(), Arc::clone(&inner)) {
            {
                let mut p = inner.payload.lock().unwrap();
                p.status = "failed".into();
                p.error = Some(e);
            }
            emit_payload(&app, &inner);
        }
    }
}

fn kill_inner(inner: &TaskInner) {
    let pid = inner.pid.load(Ordering::SeqCst);
    if pid != 0 {
        kill_process_tree(pid);
    }
    if let Some(c) = inner.child.lock().unwrap().as_mut() {
        let _ = c.start_kill();
    }
}

#[tauri::command]
pub async fn start_task(
    app: AppHandle,
    state: State<'_, TaskManager>,
    id: String,
    mut task: NewTask,
) -> Result<(), String> {
    if state.tasks.lock().unwrap().contains_key(&id) {
        return Err("任务已存在".into());
    }
    let settings = settings_snapshot(&app);
    apply_settings(&mut task, &settings);
    let inner = Arc::new(TaskInner {
        payload: Mutex::new(TaskPayload {
            id: id.clone(),
            url: task.url.clone(),
            title: None,
            status: "queued".into(),
            downloaded: 0,
            total: 0,
            speed: 0.0,
            eta: 0.0,
            file_path: None,
            error: None,
            request: task,
        }),
        child: Mutex::new(None),
        pid: AtomicU32::new(0),
        stderr_tail: Mutex::new(VecDeque::new()),
        canceled: AtomicBool::new(false),
        args: Mutex::new(Vec::new()),
    });
    state.tasks.lock().unwrap().insert(id.clone(), Arc::clone(&inner));
    state.order.lock().unwrap().insert(0, id);
    emit_payload(&app, &inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn cancel_task(app: AppHandle, state: State<'_, TaskManager>, id: String) -> Result<(), String> {
    let inner = state
        .tasks
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "任务不存在".to_string())?;
    inner.canceled.store(true, Ordering::SeqCst);
    {
        let mut p = inner.payload.lock().unwrap();
        p.status = "canceled".into();
        p.speed = 0.0;
    }
    emit_payload(&app, &inner);
    kill_inner(&inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn pause_task(app: AppHandle, state: State<'_, TaskManager>, id: String) -> Result<(), String> {
    let inner = state
        .tasks
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "任务不存在".to_string())?;
    {
        let mut p = inner.payload.lock().unwrap();
        if p.status == "queued" {
            p.status = "paused".into();
        } else if running_status(&p.status) {
            p.status = "paused".into();
            p.speed = 0.0;
            p.request.resume = Some(true);
        }
    }
    emit_payload(&app, &inner);
    kill_inner(&inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn resume_task(app: AppHandle, state: State<'_, TaskManager>, id: String) -> Result<(), String> {
    let inner = state
        .tasks
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "任务不存在".to_string())?;
    {
        let mut p = inner.payload.lock().unwrap();
        p.request.resume = Some(true);
        p.status = "queued".into();
        p.error = None;
    }
    inner.canceled.store(false, Ordering::SeqCst);
    emit_payload(&app, &inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn remove_task(app: AppHandle, state: State<'_, TaskManager>, id: String) -> Result<(), String> {
    let inner = state.tasks.lock().unwrap().get(&id).cloned();
    if let Some(inner) = inner {
        let st = inner.payload.lock().unwrap().status.clone();
        if running_status(&st) || st == "queued" {
            inner.canceled.store(true, Ordering::SeqCst);
            kill_inner(&inner);
        }
    }
    state.tasks.lock().unwrap().remove(&id);
    state.order.lock().unwrap().retain(|x| x != &id);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, TaskManager>) -> Result<Vec<TaskPayload>, String> {
    let order = state.order.lock().unwrap().clone();
    let map = state.tasks.lock().unwrap();
    let mut out = Vec::new();
    for id in order {
        if let Some(t) = map.get(&id) {
            out.push(t.payload.lock().unwrap().clone());
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        cmd.creation_flags(0x0800_0000);
        cmd.arg(format!("/select,{path}"))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(dir) = std::path::Path::new(&path).parent() {
            std::process::Command::new("xdg-open")
                .arg(dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::oldest_queued;

    #[test]
    fn queue_starts_oldest_first() {
        let order = vec!["new".into(), "mid".into(), "old".into()];
        let picked = oldest_queued(&order, |id| id == "old" || id == "mid" || id == "new");
        assert_eq!(picked, Some("old"));
        let picked = oldest_queued(&order, |id| id == "mid");
        assert_eq!(picked, Some("mid"));
        assert_eq!(oldest_queued(&order, |_| false), None);
    }
}
