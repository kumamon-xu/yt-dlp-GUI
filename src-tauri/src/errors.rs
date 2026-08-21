//! stderr 错误 → 用户友好提示（文档 §7.6 映射表，M2 下载同样复用）

pub fn friendly_error(stderr: &str) -> String {
    let t = stderr.trim();
    let lower = t.to_lowercase();
    // 取最后 3 行作为兜底原文
    let tail: String = t.lines().rev().take(3).collect::<Vec<_>>().join(" ");

    if lower.contains("age confirmation") || lower.contains("sign in to confirm your age") {
        "需要年龄验证：请在设置中导入浏览器 Cookies".into()
    } else if lower.contains("sign in to view full functionality") {
        "未登录：高清内容需要登录，请在设置中导入浏览器 Cookies".into()
    } else if lower.contains("unable to load cookies") || lower.contains("unable to read cookies") {
        "读取 Cookies 失败：请关闭目标浏览器（Edge/Chrome）后重试".into()
    } else if lower.contains("no video formats") {
        "未找到可下载的视频流：可能需要登录、地区限制，或链接已失效".into()
    } else if lower.contains("unable to extract") {
        "无法解析该链接：可能需要登录（Cookies）、地区限制，或平台反爬升级".into()
    } else if lower.contains("private video") {
        "私密视频：需要登录（Cookies）且账号有访问权限".into()
    } else if lower.contains("404") || lower.contains("not found") || lower.contains("does not exist") {
        "链接不存在或已删除（404）".into()
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "网络超时：请检查网络，或在设置中配置代理".into()
    } else if lower.contains("unable to download") && lower.contains("proxy") {
        "代理连接失败：请检查代理设置".into()
    } else {
        format!("解析失败：{tail}")
    }
}
