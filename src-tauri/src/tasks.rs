//! 任务管理：并发队列、generation 所有权、杀进程树、进度事件

use crate::command::{build_args, NewTask, TaskKind};
use crate::parser::{classify_error, parse_progress, FILE_PREFIX};
use crate::{find_engine, find_ffmpeg, js_runtime_arg, kill_process_tree, no_window_cmd, AppState};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    Starting,
    Downloading,
    Postprocess,
    Paused,
    Done,
    Failed,
    Canceled,
}

impl TaskStatus {
    pub fn is_live(self) -> bool {
        matches!(
            self,
            TaskStatus::Starting | TaskStatus::Downloading | TaskStatus::Postprocess
        )
    }
    pub fn is_running_download(self) -> bool {
        matches!(self, TaskStatus::Downloading | TaskStatus::Postprocess)
    }
    #[allow(dead_code)]
    pub fn can_enter(self, next: TaskStatus) -> bool {
        use TaskStatus::*;
        match (self, next) {
            (a, b) if a == b => true,
            (Queued, Starting | Paused | Canceled) => true,
            (Starting, Downloading | Failed | Paused | Canceled) => true,
            (Downloading, Postprocess | Paused | Canceled | Failed | Done) => true,
            (Postprocess, Done | Failed | Paused | Canceled) => true,
            (Paused, Queued | Canceled) => true,
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskPayload {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub status: TaskStatus,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub eta: f64,
    pub file_path: Option<String>,
    #[serde(default)]
    pub output_files: Vec<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    pub request: NewTask,
    #[serde(default)]
    pub kind: TaskKind,
}

#[derive(Serialize, Deserialize, Clone)]
struct TaskSnapshot {
    payload: TaskPayload,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueFile {
    schema_version: u32,
    tasks: Vec<TaskSnapshot>,
}

pub struct TaskInner {
    pub payload: Mutex<TaskPayload>,
    pub child: Mutex<Option<Child>>,
    pub pid: AtomicU32,
    pub run_generation: AtomicU64,
    /// Guards generation bump + pid + child as one critical section.
    run_mu: Mutex<()>,
    stderr_tail: Mutex<VecDeque<String>>,
    pub canceled: AtomicBool,
    args: Mutex<Vec<String>>,
}

pub struct TaskManager {
    pub tasks: Mutex<HashMap<String, Arc<TaskInner>>>,
    pub order: Mutex<Vec<String>>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
        }
    }
}

type QueueLocks<'a> = (
    std::sync::MutexGuard<'a, HashMap<String, Arc<TaskInner>>>,
    std::sync::MutexGuard<'a, Vec<String>>,
);

fn lock_maps(mgr: &TaskManager) -> QueueLocks<'_> {
    let tasks = mgr.tasks.lock().unwrap();
    let order = mgr.order.lock().unwrap();
    (tasks, order)
}

fn oldest_queued(order: &[String], is_queued: impl Fn(&str) -> bool) -> Option<&str> {
    order
        .iter()
        .rev()
        .map(|s| s.as_str())
        .find(|id| is_queued(id))
}

fn live_count(map: &HashMap<String, Arc<TaskInner>>) -> u32 {
    map.values()
        .filter(|t| t.payload.lock().unwrap().status.is_live())
        .count() as u32
}

/// Atomically claim the oldest queued task (queued → starting).
pub fn claim_next(mgr: &TaskManager, cap: u32) -> Option<Arc<TaskInner>> {
    let (map, order) = lock_maps(mgr);
    if live_count(&map) >= cap {
        return None;
    }
    let id = oldest_queued(&order, |id| {
        map.get(id)
            .map(|t| t.payload.lock().unwrap().status == TaskStatus::Queued)
            .unwrap_or(false)
    })?
    .to_string();
    let inner = map.get(&id).cloned()?;
    inner.payload.lock().unwrap().status = TaskStatus::Starting;
    Some(inner)
}

/// Old run must not mutate a newer generation.
pub fn apply_child_exit(inner: &TaskInner, gen: u64, success: bool) -> bool {
    let _run = inner.run_mu.lock().unwrap();
    if inner.run_generation.load(Ordering::SeqCst) != gen {
        return false;
    }
    let canceled = inner.canceled.load(Ordering::SeqCst);
    {
        let mut p = inner.payload.lock().unwrap();
        match p.status {
            TaskStatus::Paused
            | TaskStatus::Canceled
            | TaskStatus::Queued
            | TaskStatus::Starting => {}
            _ => {
                p.speed = 0.0;
                if canceled {
                    p.status = TaskStatus::Canceled;
                } else if success {
                    p.status = TaskStatus::Done;
                } else {
                    p.status = TaskStatus::Failed;
                    let tail: String = inner
                        .stderr_tail
                        .lock()
                        .unwrap()
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n");
                    let err = classify_error(&tail);
                    p.error_code = Some(err.code.to_string());
                    p.error = Some(err.title);
                }
            }
        }
    }
    inner.pid.store(0, Ordering::SeqCst);
    let _ = inner.child.lock().unwrap().take();
    true
}

fn persist(app: &AppHandle) {
    let mgr = app.state::<TaskManager>();
    let mut snaps = Vec::new();
    let (map, order) = lock_maps(mgr.inner());
    for id in order.iter() {
        if let Some(t) = map.get(id) {
            snaps.push(TaskSnapshot {
                payload: t.payload.lock().unwrap().clone(),
            });
        }
    }
    drop(map);
    let path = crate::config::queue_path(app);
    let file = QueueFile {
        schema_version: 1,
        tasks: snaps,
    };
    let _ = crate::fsutil::atomic_write_json(&path, &file);
}

fn parse_queue_text(text: &str) -> Vec<TaskSnapshot> {
    if let Ok(file) = serde_json::from_str::<QueueFile>(text) {
        return file.tasks;
    }
    serde_json::from_str::<Vec<TaskSnapshot>>(text).unwrap_or_default()
}

pub fn restore_queue(app: &AppHandle) {
    let path = crate::config::queue_path(app);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let snaps = parse_queue_text(&text);
    let mgr = app.state::<TaskManager>();
    let (mut map, mut order) = lock_maps(mgr.inner());
    for snap in snaps {
        let mut p = snap.payload;
        if p.status.is_live() {
            p.status = TaskStatus::Paused;
            p.speed = 0.0;
        }
        order.push(p.id.clone());
        map.insert(
            p.id.clone(),
            Arc::new(TaskInner {
                payload: Mutex::new(p),
                child: Mutex::new(None),
                pid: AtomicU32::new(0),
                run_generation: AtomicU64::new(0),
                run_mu: Mutex::new(()),
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

async fn read_stdout(
    stdout: tokio::process::ChildStdout,
    app: Option<AppHandle>,
    inner: Arc<TaskInner>,
    gen: u64,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if inner.run_generation.load(Ordering::SeqCst) != gen {
            break;
        }
        if inner.canceled.load(Ordering::SeqCst) {
            break;
        }
        if let Some((status, dl, total, speed, eta, title)) = parse_progress(&line) {
            let new_status = if status == "finished" {
                TaskStatus::Postprocess
            } else {
                TaskStatus::Downloading
            };
            {
                if inner.run_generation.load(Ordering::SeqCst) != gen {
                    break;
                }
                let mut p = inner.payload.lock().unwrap();
                if !matches!(p.status, TaskStatus::Canceled | TaskStatus::Paused) {
                    p.status = new_status;
                }
                p.downloaded = dl;
                p.total = total;
                p.speed = speed;
                p.eta = eta;
                if !title.is_empty() {
                    p.title = Some(title);
                }
            }
            if let Some(app) = &app {
                emit_payload(app, &inner);
            }
        } else if let Some(path) = line.strip_prefix(FILE_PREFIX) {
            if inner.run_generation.load(Ordering::SeqCst) != gen {
                break;
            }
            let path = path.trim().to_string();
            {
                let mut p = inner.payload.lock().unwrap();
                p.file_path = Some(path.clone());
                if !p.output_files.contains(&path) {
                    p.output_files.push(path);
                }
            }
            if let Some(app) = &app {
                emit_payload(app, &inner);
            }
        } else if !line.trim().is_empty() {
            if let Some(app) = &app {
                let id = inner.payload.lock().unwrap().id.clone();
                let _ = app.emit("task_log", serde_json::json!({ "id": id, "line": line }));
            }
        }
    }
}

async fn read_stderr(
    stderr: tokio::process::ChildStderr,
    app: Option<AppHandle>,
    inner: Arc<TaskInner>,
    gen: u64,
) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if inner.run_generation.load(Ordering::SeqCst) != gen {
            break;
        }
        {
            let mut tail = inner.stderr_tail.lock().unwrap();
            tail.push_back(line.clone());
            if tail.len() > 10 {
                tail.pop_front();
            }
        }
        if !line.trim().is_empty() {
            if let Some(app) = &app {
                let id = inner.payload.lock().unwrap().id.clone();
                let _ = app.emit(
                    "task_log",
                    serde_json::json!({ "id": id, "line": format!("[stderr] {line}") }),
                );
            }
        }
    }
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
    if task
        .out_template
        .as_ref()
        .map(|x| x.is_empty())
        .unwrap_or(true)
        && !s.out_template.is_empty()
    {
        task.out_template = Some(s.out_template.clone());
    }
    if task.concurrent_fragments.is_none() {
        task.concurrent_fragments = Some(s.concurrent_fragments);
    }
    if task
        .limit_rate
        .as_ref()
        .map(|x| x.is_empty())
        .unwrap_or(true)
    {
        task.limit_rate = s.limit_rate.clone();
    }
    if task
        .cookies_file
        .as_ref()
        .map(|x| x.is_empty())
        .unwrap_or(true)
    {
        task.cookies_file = s.cookies_file.clone();
    }
    if task
        .cookies_browser
        .as_ref()
        .map(|x| x.is_empty())
        .unwrap_or(true)
    {
        task.cookies_browser = s.cookies_browser.clone();
    }
    if task.proxy.as_ref().map(|x| x.is_empty()).unwrap_or(true) {
        task.proxy = s.proxy.clone();
    }
    if task
        .merge_format
        .as_ref()
        .map(|x| x.is_empty())
        .unwrap_or(true)
    {
        task.merge_format = Some(s.merge_format.clone());
    }
}

/// Spawn a download process. Returns the generation number.
pub fn spawn_download_with(
    inner: Arc<TaskInner>,
    engine: &Path,
    ffmpeg: Option<&Path>,
    app: Option<AppHandle>,
) -> Result<u64, String> {
    crate::validate::validate_concurrent_fragments(
        inner
            .payload
            .lock()
            .unwrap()
            .request
            .concurrent_fragments
            .unwrap_or(4),
    )?;
    crate::validate::validate_limit_rate(
        inner.payload.lock().unwrap().request.limit_rate.as_deref(),
    )?;
    crate::validate::validate_proxy(inner.payload.lock().unwrap().request.proxy.as_deref())?;
    crate::validate::validate_playlist_items(
        inner
            .payload
            .lock()
            .unwrap()
            .request
            .playlist_items
            .as_deref(),
    )?;

    let request = inner.payload.lock().unwrap().request.clone();
    let mut cfg = request.to_config();
    cfg.ffmpeg_location = ffmpeg.map(|p| p.to_string_lossy().into());
    cfg.js_runtime = js_runtime_arg();
    if inner.payload.lock().unwrap().status == TaskStatus::Paused || request.resume.unwrap_or(false)
    {
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

    {
        let p = inner.payload.lock().unwrap();
        if inner.canceled.load(Ordering::SeqCst)
            || matches!(p.status, TaskStatus::Paused | TaskStatus::Canceled)
        {
            return Err("任务已取消或暂停".into());
        }
    }

    let gen = {
        let _run = inner.run_mu.lock().unwrap();
        let gen = inner.run_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let old = inner.child.lock().unwrap().take();
        if let Some(mut old) = old {
            let _ = old.start_kill();
        }
        gen
    };

    let mut cmd = no_window_cmd(engine);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("stdout 管道失败")?;
    let stderr = child.stderr.take().ok_or("stderr 管道失败")?;
    let pid = child.id().unwrap_or(0);

    let abort_child = {
        let _run = inner.run_mu.lock().unwrap();
        if inner.run_generation.load(Ordering::SeqCst) != gen {
            Some(child)
        } else {
            let mut p = inner.payload.lock().unwrap();
            let abort =
                inner.canceled.load(Ordering::SeqCst) || !matches!(p.status, TaskStatus::Starting);
            if abort {
                if p.status == TaskStatus::Starting && inner.canceled.load(Ordering::SeqCst) {
                    p.status = TaskStatus::Canceled;
                }
                Some(child)
            } else {
                inner.pid.store(pid, Ordering::SeqCst);
                *inner.child.lock().unwrap() = Some(child);
                p.status = TaskStatus::Downloading;
                p.error = None;
                p.error_code = None;
                p.request.resume = Some(cfg.resume);
                None
            }
        }
    };
    if let Some(mut child) = abort_child {
        let _ = child.start_kill();
        if pid != 0 {
            kill_process_tree(pid);
        }
        return Err("任务已取消或暂停".into());
    }
    if let Some(app) = &app {
        emit_payload(app, &inner);
    }

    let app1 = app.clone();
    let app2 = app.clone();
    let app3 = app.clone();
    let i1 = Arc::clone(&inner);
    let i2 = Arc::clone(&inner);
    let i3 = Arc::clone(&inner);
    tokio::spawn(async move { read_stdout(stdout, app1, i1, gen).await });
    tokio::spawn(async move { read_stderr(stderr, app2, i2, gen).await });
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            if i3.run_generation.load(Ordering::SeqCst) != gen {
                break;
            }
            let (superseded, status) = {
                let mut g = i3.child.lock().unwrap();
                if i3.run_generation.load(Ordering::SeqCst) != gen {
                    (true, None)
                } else {
                    match g.as_mut() {
                        Some(c) => (false, c.try_wait().ok().flatten()),
                        None => (true, None),
                    }
                }
            };
            if superseded {
                break;
            }
            if let Some(st) = status {
                let owned = apply_child_exit(&i3, gen, st.success());
                if owned {
                    if let Some(app) = &app3 {
                        emit_payload(app, &i3);
                        persist(app);
                        pump_queue(app.clone());
                    }
                }
                break;
            }
        }
    });
    Ok(gen)
}

fn spawn_download(app: AppHandle, inner: Arc<TaskInner>) -> Result<u64, String> {
    let settings = settings_snapshot(&app);
    let mut request = inner.payload.lock().unwrap().request.clone();
    apply_settings(&mut request, &settings);
    inner.payload.lock().unwrap().request = request.clone();
    let engine = find_engine(settings.engine_path.as_deref())?;
    let ffmpeg = find_ffmpeg(settings.ffmpeg_path.as_deref()).ok();
    spawn_download_with(inner, &engine, ffmpeg.as_deref(), Some(app))
}

fn max_concurrent(app: &AppHandle) -> u32 {
    app.state::<AppState>()
        .settings
        .lock()
        .ok()
        .map(|s| s.max_concurrent_tasks.clamp(1, 8))
        .unwrap_or(2)
}

fn pump_queue(app: AppHandle) {
    let cap = max_concurrent(&app);
    loop {
        let next = {
            let mgr = app.state::<TaskManager>();
            claim_next(mgr.inner(), cap)
        };
        let Some(inner) = next else { return };
        if let Err(e) = spawn_download(app.clone(), Arc::clone(&inner)) {
            {
                let mut p = inner.payload.lock().unwrap();
                if p.status == TaskStatus::Starting {
                    p.status = TaskStatus::Failed;
                    p.error = Some(e);
                    p.error_code = Some("PROCESS_FAILED".into());
                }
            }
            emit_payload(&app, &inner);
            persist(&app);
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

fn empty_payload(id: String, task: NewTask) -> TaskPayload {
    let kind = task.kind.unwrap_or_else(|| {
        if task.write_subs.unwrap_or(false) && task.skip_download.unwrap_or(false) {
            TaskKind::Subtitles
        } else if task.write_thumbnail.unwrap_or(false) && task.skip_download.unwrap_or(false) {
            TaskKind::Thumbnail
        } else if task.write_info_json.unwrap_or(false) && task.skip_download.unwrap_or(false) {
            TaskKind::Metadata
        } else if task.preset == "mp3" || task.preset == "m4a" {
            TaskKind::Audio
        } else {
            TaskKind::Video
        }
    });
    TaskPayload {
        id,
        url: task.url.clone(),
        title: None,
        status: TaskStatus::Queued,
        downloaded: 0,
        total: 0,
        speed: 0.0,
        eta: 0.0,
        file_path: None,
        output_files: Vec::new(),
        error: None,
        error_code: None,
        request: task,
        kind,
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
    crate::validate::validate_concurrent_fragments(task.concurrent_fragments.unwrap_or(4))?;
    crate::validate::validate_limit_rate(task.limit_rate.as_deref())?;
    crate::validate::validate_proxy(task.proxy.as_deref())?;
    crate::validate::validate_playlist_items(task.playlist_items.as_deref())?;
    let settings = settings_snapshot(&app);
    apply_settings(&mut task, &settings);
    let inner = Arc::new(TaskInner {
        payload: Mutex::new(empty_payload(id.clone(), task)),
        child: Mutex::new(None),
        pid: AtomicU32::new(0),
        run_generation: AtomicU64::new(0),
        run_mu: Mutex::new(()),
        stderr_tail: Mutex::new(VecDeque::new()),
        canceled: AtomicBool::new(false),
        args: Mutex::new(Vec::new()),
    });
    {
        let (mut map, mut order) = lock_maps(state.inner());
        map.insert(id.clone(), Arc::clone(&inner));
        order.insert(0, id);
    }
    emit_payload(&app, &inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn cancel_task(
    app: AppHandle,
    state: State<'_, TaskManager>,
    id: String,
) -> Result<(), String> {
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
        p.status = TaskStatus::Canceled;
        p.speed = 0.0;
    }
    emit_payload(&app, &inner);
    kill_inner(&inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn pause_task(
    app: AppHandle,
    state: State<'_, TaskManager>,
    id: String,
) -> Result<(), String> {
    let inner = state
        .tasks
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "任务不存在".to_string())?;
    {
        let mut p = inner.payload.lock().unwrap();
        if p.status == TaskStatus::Queued || p.status == TaskStatus::Starting {
            p.status = TaskStatus::Paused;
        } else if p.status.is_running_download() {
            p.status = TaskStatus::Paused;
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
pub async fn resume_task(
    app: AppHandle,
    state: State<'_, TaskManager>,
    id: String,
) -> Result<(), String> {
    let inner = state
        .tasks
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "任务不存在".to_string())?;
    {
        let mut p = inner.payload.lock().unwrap();
        if p.status != TaskStatus::Paused {
            return Err("只能恢复已暂停的任务".into());
        }
        p.request.resume = Some(true);
        p.status = TaskStatus::Queued;
        p.error = None;
        p.error_code = None;
    }
    inner.canceled.store(false, Ordering::SeqCst);
    emit_payload(&app, &inner);
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn remove_task(
    app: AppHandle,
    state: State<'_, TaskManager>,
    id: String,
) -> Result<(), String> {
    let inner = state.tasks.lock().unwrap().get(&id).cloned();
    if let Some(inner) = inner {
        let st = inner.payload.lock().unwrap().status;
        if st.is_live() || st == TaskStatus::Queued {
            inner.canceled.store(true, Ordering::SeqCst);
            kill_inner(&inner);
        }
    }
    {
        let (mut map, mut order) = lock_maps(state.inner());
        map.remove(&id);
        order.retain(|x| x != &id);
    }
    persist(&app);
    pump_queue(app);
    Ok(())
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, TaskManager>) -> Result<Vec<TaskPayload>, String> {
    let (map, order) = lock_maps(state.inner());
    let mut out = Vec::new();
    for id in order.iter() {
        if let Some(t) = map.get(id) {
            out.push(t.payload.lock().unwrap().clone());
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let target = if p.exists() {
        p.to_path_buf()
    } else if let Some(parent) = p.parent().filter(|d| d.exists()) {
        parent.to_path_buf()
    } else {
        return Err("文件不存在".into());
    };
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        cmd.creation_flags(0x0800_0000);
        if target.is_file() {
            cmd.arg(format!("/select,{}", target.display()));
        } else {
            cmd.arg(target.as_os_str());
        }
        cmd.spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if target.is_file() {
            cmd.arg("-R");
        }
        cmd.arg(&target).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = if target.is_file() {
            target.parent().unwrap_or(&target)
        } else {
            &target
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
impl NewTask {
    fn test_default() -> Self {
        NewTask {
            url: String::new(),
            preset: "mp4".into(),
            custom_format: None,
            audio_quality: None,
            merge_format: None,
            out_dir: None,
            out_template: None,
            concurrent_fragments: Some(4),
            limit_rate: None,
            cookies_browser: None,
            cookies_file: None,
            proxy: None,
            embed_thumbnail: None,
            embed_metadata: None,
            write_subs: None,
            sub_langs: None,
            embed_subs: None,
            sponsorblock: None,
            no_playlist: None,
            playlist_items: None,
            resume: None,
            skip_download: None,
            write_thumbnail: None,
            convert_subs: None,
            write_info_json: None,
            kind: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_inner(id: &str, url: &str) -> Arc<TaskInner> {
        let mut task = NewTask::test_default();
        task.url = url.into();
        task.out_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
        Arc::new(TaskInner {
            payload: Mutex::new(empty_payload(id.into(), task)),
            child: Mutex::new(None),
            pid: AtomicU32::new(0),
            run_generation: AtomicU64::new(0),
            run_mu: Mutex::new(()),
            stderr_tail: Mutex::new(VecDeque::new()),
            canceled: AtomicBool::new(false),
            args: Mutex::new(Vec::new()),
        })
    }

    fn enqueue(mgr: &TaskManager, inner: Arc<TaskInner>) {
        let id = inner.payload.lock().unwrap().id.clone();
        let (mut map, mut order) = lock_maps(mgr);
        map.insert(id.clone(), inner);
        order.insert(0, id);
    }

    #[test]
    fn queue_starts_oldest_first() {
        let order = vec!["new".into(), "mid".into(), "old".into()];
        let picked = oldest_queued(&order, |id| id == "old" || id == "mid" || id == "new");
        assert_eq!(picked, Some("old"));
        let picked = oldest_queued(&order, |id| id == "mid");
        assert_eq!(picked, Some("mid"));
        assert_eq!(oldest_queued(&order, |_| false), None);
    }

    #[test]
    fn claim_is_atomic_starting() {
        let mgr = TaskManager::default();
        for i in 0..10 {
            enqueue(&mgr, make_inner(&format!("t{i}"), "https://example.com"));
        }
        let a = claim_next(&mgr, 2).expect("first");
        let b = claim_next(&mgr, 2).expect("second");
        assert!(claim_next(&mgr, 2).is_none(), "cap 2");
        assert_ne!(a.payload.lock().unwrap().id, b.payload.lock().unwrap().id);
        assert_eq!(a.payload.lock().unwrap().status, TaskStatus::Starting);
        assert_eq!(b.payload.lock().unwrap().status, TaskStatus::Starting);
        let ids: std::collections::HashSet<_> = mgr
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.payload.lock().unwrap().status == TaskStatus::Starting)
            .map(|t| t.payload.lock().unwrap().id.clone())
            .collect();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn stale_generation_cannot_fail_or_clear_pid() {
        let inner = make_inner("t", "https://example.com");
        inner.run_generation.store(1, Ordering::SeqCst);
        inner.pid.store(111, Ordering::SeqCst);
        inner.payload.lock().unwrap().status = TaskStatus::Downloading;

        // new run
        inner.run_generation.store(2, Ordering::SeqCst);
        inner.pid.store(222, Ordering::SeqCst);
        inner.payload.lock().unwrap().downloaded = 50;

        assert!(!apply_child_exit(&inner, 1, false));
        assert_eq!(
            inner.payload.lock().unwrap().status,
            TaskStatus::Downloading
        );
        assert_eq!(inner.pid.load(Ordering::SeqCst), 222);
        assert_eq!(inner.payload.lock().unwrap().downloaded, 50);

        assert!(apply_child_exit(&inner, 2, true));
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Done);
        assert_eq!(inner.pid.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn pause_then_resume_old_exit_ignored() {
        let inner = make_inner("t", "https://example.com");
        inner.run_generation.store(1, Ordering::SeqCst);
        inner.pid.store(1, Ordering::SeqCst);
        inner.payload.lock().unwrap().status = TaskStatus::Downloading;
        inner.payload.lock().unwrap().status = TaskStatus::Paused;
        inner.run_generation.store(2, Ordering::SeqCst);
        inner.pid.store(99, Ordering::SeqCst);
        inner.payload.lock().unwrap().status = TaskStatus::Downloading;
        apply_child_exit(&inner, 1, false);
        assert_eq!(
            inner.payload.lock().unwrap().status,
            TaskStatus::Downloading
        );
        assert_eq!(inner.pid.load(Ordering::SeqCst), 99);
    }

    #[test]
    fn cancel_then_retry_old_exit_ignored() {
        let inner = make_inner("t", "https://example.com");
        inner.run_generation.store(1, Ordering::SeqCst);
        inner.pid.store(1, Ordering::SeqCst);
        inner.payload.lock().unwrap().status = TaskStatus::Canceled;
        inner.run_generation.store(2, Ordering::SeqCst);
        inner.pid.store(77, Ordering::SeqCst);
        inner.payload.lock().unwrap().status = TaskStatus::Downloading;
        inner.payload.lock().unwrap().downloaded = 12;
        apply_child_exit(&inner, 1, false);
        assert_eq!(
            inner.payload.lock().unwrap().status,
            TaskStatus::Downloading
        );
        assert_eq!(inner.pid.load(Ordering::SeqCst), 77);
        assert_eq!(inner.payload.lock().unwrap().downloaded, 12);
    }

    #[test]
    fn status_machine_rejects_illegal_jumps() {
        assert!(TaskStatus::Queued.can_enter(TaskStatus::Starting));
        assert!(TaskStatus::Downloading.can_enter(TaskStatus::Paused));
        assert!(!TaskStatus::Done.can_enter(TaskStatus::Downloading));
        assert!(!TaskStatus::Canceled.can_enter(TaskStatus::Queued));
    }

    #[test]
    fn parse_queue_v1_and_legacy_array() {
        let legacy = r#"[{"payload":{"id":"a","url":"u","title":null,"status":"downloading","downloaded":0,"total":0,"speed":0,"eta":0,"filePath":null,"error":null,"request":{"url":"u","preset":"mp4"}}}]"#;
        let snaps = parse_queue_text(legacy);
        assert_eq!(snaps.len(), 1);
        let mut p = snaps[0].payload.clone();
        if p.status.is_live() {
            p.status = TaskStatus::Paused;
        }
        assert_eq!(p.status, TaskStatus::Paused);
        let v1 = r#"{"schemaVersion":1,"tasks":[]}"#;
        assert!(parse_queue_text(v1).is_empty());
    }

    #[test]
    fn concurrent_claims_never_double() {
        let mgr = Arc::new(TaskManager::default());
        for i in 0..10 {
            enqueue(&mgr, make_inner(&format!("c{i}"), "https://example.com"));
        }
        let mut handles = vec![];
        for _ in 0..8 {
            let mgr = Arc::clone(&mgr);
            handles.push(std::thread::spawn(move || claim_next(&mgr, 2)));
        }
        let got: Vec<_> = handles
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect();
        assert!(got.len() <= 2);
        let ids: std::collections::HashSet<_> = got
            .iter()
            .map(|t| t.payload.lock().unwrap().id.clone())
            .collect();
        assert_eq!(ids.len(), got.len());
    }

    #[test]
    fn open_folder_missing_errors() {
        let err = open_folder("Z:/definitely-missing-ytdlp-gui/nope.mp4".into());
        assert!(err.is_err());
    }

    fn rustc_bin() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO"));
        p.set_file_name(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        p
    }

    fn compile_fake(dir: &Path, name: &str, sleep_ms: u64) -> PathBuf {
        let src = dir.join(format!("{name}.rs"));
        std::fs::write(
            &src,
            format!(
                r#"
fn main() {{
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {{
        println!("yt-dlp 0.0-fake");
        return;
    }}
    println!("YDLP|downloading|10|100|1|1|t");
    std::thread::sleep(std::time::Duration::from_millis({sleep_ms}));
    println!("YDLPFILE|/tmp/out.mp4");
}}
"#
            ),
        )
        .unwrap();
        let exe = dir.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        });
        let st = std::process::Command::new(rustc_bin())
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .status()
            .expect("rustc");
        assert!(st.success(), "rustc fake engine");
        exe
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fake_process_stale_wait_does_not_clobber() {
        let dir = std::env::temp_dir().join(format!(
            "fake-dlp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let slow = compile_fake(&dir, "slow", 400);
        let fast = compile_fake(&dir, "fast", 50);
        let inner = make_inner("t", "https://example.com");
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        spawn_download_with(Arc::clone(&inner), &slow, None, None).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        spawn_download_with(Arc::clone(&inner), &fast, None, None).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let p = inner.payload.lock().unwrap().clone();
        assert_ne!(
            p.status,
            TaskStatus::Failed,
            "stale wait must not fail new run"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_fake_pumps_respect_cap() {
        let dir = std::env::temp_dir().join(format!(
            "fake-cap-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = compile_fake(&dir, "cap", 250);
        let mgr = TaskManager::default();
        for i in 0..6 {
            enqueue(&mgr, make_inner(&format!("c{i}"), "https://example.com"));
        }
        let mut spawned = 0u32;
        loop {
            let Some(inner) = claim_next(&mgr, 2) else {
                break;
            };
            spawn_download_with(Arc::clone(&inner), &engine, None, None).unwrap();
            spawned += 1;
        }
        assert_eq!(spawned, 2, "cap 2");
        {
            let map = mgr.tasks.lock().unwrap();
            let live = map
                .values()
                .filter(|t| t.payload.lock().unwrap().status.is_live())
                .count();
            let queued = map
                .values()
                .filter(|t| t.payload.lock().unwrap().status == TaskStatus::Queued)
                .count();
            assert_eq!(live, 2);
            assert_eq!(queued, 4);
        }
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let live2 = mgr
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.payload.lock().unwrap().status.is_live())
            .count();
        assert!(live2 <= 2, "never more than cap live, got {live2}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spawn_missing_engine_fails() {
        let inner = make_inner("t", "https://example.com");
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        let missing = if cfg!(windows) {
            Path::new(r"Z:\definitely-missing-ytdlp-gui\yt-dlp.exe")
        } else {
            Path::new("/definitely-missing-ytdlp-gui/yt-dlp")
        };
        let err = spawn_download_with(Arc::clone(&inner), missing, None, None);
        assert!(err.is_err());
        inner.payload.lock().unwrap().status = TaskStatus::Failed;
        let mgr = TaskManager::default();
        enqueue(&mgr, Arc::clone(&inner));
        enqueue(&mgr, make_inner("next", "https://example.com"));
        let nxt = claim_next(&mgr, 2).expect("queue continues after spawn fail");
        assert_eq!(nxt.payload.lock().unwrap().id, "next");
    }

    #[test]
    fn lock_order_claim_and_snapshot_no_deadlock() {
        let mgr = Arc::new(TaskManager::default());
        for i in 0..8 {
            enqueue(&mgr, make_inner(&format!("d{i}"), "https://example.com"));
        }
        let mut handles = vec![];
        for _ in 0..6 {
            let mgr = Arc::clone(&mgr);
            handles.push(std::thread::spawn(move || {
                for _ in 0..80 {
                    let _ = claim_next(&mgr, 4);
                    let (map, order) = lock_maps(&mgr);
                    let _ = order.len() + map.len();
                }
            }));
        }
        for h in handles {
            h.join().expect("no deadlock");
        }
    }

    #[tokio::test]
    async fn spawn_does_not_promote_already_canceled() {
        let dir = std::env::temp_dir().join(format!(
            "fake-precancel-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = compile_fake(&dir, "precancel", 200);
        let inner = make_inner("t", "https://example.com");
        inner.canceled.store(true, Ordering::SeqCst);
        inner.payload.lock().unwrap().status = TaskStatus::Canceled;
        let res = spawn_download_with(Arc::clone(&inner), &engine, None, None);
        assert!(res.is_err());
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Canceled);
        assert_eq!(inner.pid.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spawn_does_not_promote_already_paused() {
        let dir = std::env::temp_dir().join(format!(
            "fake-prepause-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = compile_fake(&dir, "prepause", 200);
        let inner = make_inner("t", "https://example.com");
        inner.payload.lock().unwrap().status = TaskStatus::Paused;
        let res = spawn_download_with(Arc::clone(&inner), &engine, None, None);
        assert!(res.is_err());
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Paused);
        assert_eq!(inner.pid.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_abort_if_canceled_during_starting() {
        let dir = std::env::temp_dir().join(format!(
            "fake-abort-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = compile_fake(&dir, "abort", 400);
        let inner = make_inner("t", "https://example.com");
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        let watcher = Arc::clone(&inner);
        let h = std::thread::spawn(move || {
            while watcher.run_generation.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            watcher.canceled.store(true, Ordering::SeqCst);
            watcher.payload.lock().unwrap().status = TaskStatus::Canceled;
        });
        let res = spawn_download_with(Arc::clone(&inner), &engine, None, None);
        let _ = h.join();
        assert!(res.is_err(), "spawn must abort after cancel");
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Canceled);
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Canceled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_abort_if_paused_during_starting() {
        let dir = std::env::temp_dir().join(format!(
            "fake-pause-start-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = compile_fake(&dir, "pstart", 400);
        let inner = make_inner("t", "https://example.com");
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        let watcher = Arc::clone(&inner);
        let h = std::thread::spawn(move || {
            while watcher.run_generation.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            watcher.payload.lock().unwrap().status = TaskStatus::Paused;
        });
        let res = spawn_download_with(Arc::clone(&inner), &engine, None, None);
        let _ = h.join();
        assert!(res.is_err(), "spawn must abort after pause");
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Paused);
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert_eq!(inner.payload.lock().unwrap().status, TaskStatus::Paused);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pause_then_resume_new_pid_survives_stale_exit() {
        let dir = std::env::temp_dir().join(format!(
            "fake-pid-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let slow = compile_fake(&dir, "slow", 800);
        let fast = compile_fake(&dir, "fast", 250);
        let inner = make_inner("t", "https://example.com");
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        spawn_download_with(Arc::clone(&inner), &slow, None, None).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let old_gen = inner.run_generation.load(Ordering::SeqCst);
        let old_pid = inner.pid.load(Ordering::SeqCst);
        assert_ne!(old_pid, 0);
        inner.payload.lock().unwrap().status = TaskStatus::Paused;
        kill_inner(&inner);
        inner.payload.lock().unwrap().status = TaskStatus::Starting;
        inner.canceled.store(false, Ordering::SeqCst);
        spawn_download_with(Arc::clone(&inner), &fast, None, None).unwrap();
        let new_pid = inner.pid.load(Ordering::SeqCst);
        assert_ne!(new_pid, 0);
        assert_ne!(new_pid, old_pid);
        apply_child_exit(&inner, old_gen, false);
        assert_eq!(inner.pid.load(Ordering::SeqCst), new_pid);
        assert_eq!(
            inner.payload.lock().unwrap().status,
            TaskStatus::Downloading
        );
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let st = inner.payload.lock().unwrap().status;
        assert_ne!(st, TaskStatus::Failed);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
