import AppKit
import Carbon
import Darwin
import Security
import UserNotifications
import WebKit

private let appName = "SyncAiDync"

private enum LaunchError: LocalizedError {
    case port, key, binary, exited, timedOut

    var errorDescription: String? {
        switch self {
        case .port: return "Could not reserve a loopback port."
        case .key: return "Could not create a launch key."
        case .binary: return "The bundled server is missing."
        case .exited: return "The server stopped."
        case .timedOut: return "The server did not respond in time."
        }
    }
}

private func loopbackPort() throws -> UInt16 {
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    guard fd >= 0 else { throw LaunchError.port }
    defer { close(fd) }
    var address = sockaddr_in()
    address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
    address.sin_family = sa_family_t(AF_INET)
    address.sin_addr.s_addr = inet_addr("127.0.0.1")
    var size = socklen_t(MemoryLayout<sockaddr_in>.size)
    let result = withUnsafeMutablePointer(to: &address) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            bind(fd, $0, size) == 0 ? getsockname(fd, $0, &size) : -1
        }
    }
    guard result == 0 else { throw LaunchError.port }
    return UInt16(bigEndian: address.sin_port)
}

private func launchKey() throws -> String {
    var bytes = [UInt8](repeating: 0, count: 32)
    guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
        throw LaunchError.key
    }
    return bytes.map { String(format: "%02x", $0) }.joined()
}

private func shellPath(_ env: [String: String]) async -> String {
    let fallback = [env["PATH"], "/opt/homebrew/bin:/usr/local/bin"]
        .compactMap { $0 }.joined(separator: ":")
    let shell = Process()
    shell.executableURL = URL(fileURLWithPath: env["SHELL"] ?? "/bin/zsh")
    shell.arguments = ["-l", "-c", "printf '\\nSYNCAIDYNC_PATH=%s\\n' \"$PATH\""]
    let stdout = Pipe()
    shell.standardInput = FileHandle.nullDevice
    shell.standardOutput = stdout
    shell.standardError = FileHandle.nullDevice
    do { try shell.run() } catch { return fallback }
    let read = Task.detached { stdout.fileHandleForReading.readDataToEndOfFile() }
    let deadline = Date().addingTimeInterval(2)
    while shell.isRunning && Date() < deadline {
        try? await Task.sleep(nanoseconds: 50_000_000)
    }
    if shell.isRunning {
        kill(shell.processIdentifier, SIGKILL)
        return fallback
    }
    guard shell.terminationStatus == 0,
          let text = String(data: await read.value, encoding: .utf8),
          let path = text.split(separator: "\n").last(where: { $0.hasPrefix("SYNCAIDYNC_PATH=") })
    else { return fallback }
    let value = path.dropFirst("SYNCAIDYNC_PATH=".count)
    return value.isEmpty ? fallback : String(value)
}

private final class ServerLog: @unchecked Sendable {
    private let queue = DispatchQueue(label: "syncaidync.server-log")
    private let file: URL
    private let cap = 1024 * 1024

    init(file: URL) { self.file = file }

    func append(_ data: Data) {
        queue.async {
            do {
                let fm = FileManager.default
                try fm.createDirectory(at: self.file.deletingLastPathComponent(),
                                       withIntermediateDirectories: true)
                if !fm.fileExists(atPath: self.file.path) {
                    try Data().write(to: self.file)
                }
                let handle = try FileHandle(forUpdating: self.file)
                defer { try? handle.close() }
                let end = try handle.seekToEnd()
                if end + UInt64(data.count) > UInt64(self.cap) {
                    let count = min(Int(end), max(0, self.cap - data.count))
                    try handle.seek(toOffset: end - UInt64(count))
                    let tail = try handle.read(upToCount: count) ?? Data()
                    try handle.seek(toOffset: 0)
                    try handle.write(contentsOf: tail)
                    try handle.write(contentsOf: data.suffix(self.cap))
                    try handle.truncate(atOffset: try handle.offset())
                } else {
                    try handle.write(contentsOf: data)
                }
            } catch {
                NSLog("Could not write server log: %@", error.localizedDescription)
            }
        }
    }

    func tail() -> String {
        queue.sync {
            guard let handle = try? FileHandle(forReadingFrom: file) else { return "" }
            defer { try? handle.close() }
            let end = (try? handle.seekToEnd()) ?? 0
            try? handle.seek(toOffset: end - min(end, UInt64(cap)))
            let data = (try? handle.readToEnd()) ?? Data()
            return String(decoding: data, as: UTF8.self)
                .split(separator: "\n", omittingEmptySubsequences: false).suffix(40).joined(separator: "\n")
        }
    }
}

@MainActor
private final class App: NSObject, NSApplicationDelegate, NSWindowDelegate, WKNavigationDelegate,
                         WKUIDelegate, WKScriptMessageHandler, UNUserNotificationCenterDelegate {
    private var window: NSWindow!
    private var web: WKWebView!
    private var popouts: [NSWindow] = []
    private var server: Process?
    private var port: UInt16?
    private var launching = false
    private var terminating = false
    private var quitPending = false
    private var hotkey: EventHotKeyRef?
    private var hotkeyHandler: EventHandlerRef?
    private let log = ServerLog(file: FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Logs/SyncAiDync/server.log"))

    func applicationDidFinishLaunching(_ notification: Notification) {
        installMenus()
        UNUserNotificationCenter.current().delegate = self
        window = makeWindow(size: NSSize(width: 1280, height: 820))
        window.setFrameAutosaveName(appName)
        web = makeWebView(WKWebViewConfiguration())
        place(web, in: window)
        window.center()
        _ = window.setFrameUsingName(appName)
        showWindow()
        registerHotkey()
        start()
    }

    private func makeWindow(size: NSSize) -> NSWindow {
        let result = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        result.title = appName
        result.titlebarAppearsTransparent = true
        result.isReleasedWhenClosed = false
        result.delegate = self
        return result
    }

    private func place(_ view: WKWebView, in window: NSWindow) {
        guard let content = window.contentView else { return }
        view.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(view)
        let top = (window.contentLayoutGuide as? NSLayoutGuide)?.topAnchor ?? content.topAnchor
        NSLayoutConstraint.activate([
            view.topAnchor.constraint(equalTo: top),
            view.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            view.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            view.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])
    }

    private func makeWebView(_ config: WKWebViewConfiguration, installBridge: Bool = true) -> WKWebView {
        if installBridge {
            let scripts = config.userContentController
            scripts.addUserScript(WKUserScript(source: "window.__syncaidyncNative = true;",
                                                injectionTime: .atDocumentStart, forMainFrameOnly: true))
            scripts.add(self, name: "syncaidync")
        }
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = self
        view.uiDelegate = self
        return view
    }

    private func start() {
        guard !launching, !terminating else { return }
        launching = true
        web.loadHTMLString("<p>Starting SyncAiDync…</p>", baseURL: nil)
        Task {
            defer { launching = false }
            do {
                let port = try loopbackPort()
                let key = try launchKey()
                guard let binary = Bundle.main.url(forResource: "syncaidync", withExtension: nil) else {
                    throw LaunchError.binary
                }
                var env = ProcessInfo.processInfo.environment
                env["PATH"] = await shellPath(env)
                guard !terminating else { return }
                env["SYNCAIDYNC_KEY"] = key
                let process = Process()
                process.executableURL = binary
                process.arguments = ["serve", "--no-open", "--port", String(port),
                                     "--parent-pid", String(ProcessInfo.processInfo.processIdentifier)]
                process.environment = env
                process.standardInput = FileHandle.nullDevice
                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = pipe
                let log = self.log
                pipe.fileHandleForReading.readabilityHandler = { handle in
                    let data = handle.availableData
                    if data.isEmpty { handle.readabilityHandler = nil } else { log.append(data) }
                }
                process.terminationHandler = { [weak self] process in
                    Task { @MainActor in
                        guard let self, self.server === process, !self.terminating, !self.launching else { return }
                        self.port = nil
                        self.showError(LaunchError.exited)
                    }
                }
                try process.run()
                self.server = process
                self.port = port
                let origin = "http://127.0.0.1:\(port)"
                let client = URLSession(configuration: .ephemeral)
                defer { client.invalidateAndCancel() }
                let deadline = Date().addingTimeInterval(10)
                while Date() < deadline {
                    guard process.isRunning else { throw LaunchError.exited }
                    if let url = URL(string: origin + "/api/v1/auth") {
                        let request = URLRequest(url: url, timeoutInterval: 0.5)
                        if let (_, response) = try? await client.data(for: request),
                           let http = response as? HTTPURLResponse, [200, 401].contains(http.statusCode),
                           let launch = URL(string: origin + "/#key=" + key) {
                            web.load(URLRequest(url: launch))
                            return
                        }
                    }
                    try? await Task.sleep(nanoseconds: 120_000_000)
                }
                throw LaunchError.timedOut
            } catch {
                await stopServer()
                if !terminating { showError(error) }
            }
        }
    }

    private func stopServer() async {
        guard let process = server else { return }
        server = nil
        port = nil
        if process.isRunning { process.terminate() }
        let deadline = Date().addingTimeInterval(4)
        while process.isRunning && Date() < deadline {
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        if process.isRunning { kill(process.processIdentifier, SIGKILL) }
    }

    private func showError(_ error: Error) {
        func escape(_ text: String) -> String {
            text.replacingOccurrences(of: "&", with: "&amp;")
                .replacingOccurrences(of: "<", with: "&lt;")
                .replacingOccurrences(of: ">", with: "&gt;")
        }
        web.loadHTMLString("""
        <!doctype html><meta charset="utf-8">
        <style>body{font:16px system-ui;padding:48px}pre{white-space:pre-wrap}</style>
        <h1>SyncAiDync could not start</h1><p>\(escape(error.localizedDescription))</p>
        <p><a href="syncaidync-local:retry">Try again</a></p><pre>\(escape(log.tail()))</pre>
        """, baseURL: nil)
    }

    private func local(_ url: URL) -> Bool {
        url.scheme == "http" && url.host == "127.0.0.1" && url.port == port.map(Int.init) && url.path == "/"
    }

    func webView(_ view: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url else { decisionHandler(.cancel); return }
        if url.absoluteString == "syncaidync-local:retry", port == nil {
            decisionHandler(.cancel)
            start()
        } else if local(url) || url.absoluteString == "about:blank" {
            decisionHandler(.allow)
        } else {
            decisionHandler(.cancel)
            if action.targetFrame != nil && ["http", "https", "mailto"].contains(url.scheme ?? "") {
                NSWorkspace.shared.open(url)
            }
        }
    }

    func webView(_ view: WKWebView, createWebViewWith config: WKWebViewConfiguration,
                 for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        guard action.targetFrame == nil, let url = action.request.url else { return nil }
        guard local(url) else {
            if ["http", "https", "mailto"].contains(url.scheme ?? "") { NSWorkspace.shared.open(url) }
            return nil
        }
        let panel = makeWindow(size: NSSize(width: 900, height: 700))
        let child = makeWebView(config, installBridge: false)
        place(child, in: panel)
        panel.center()
        popouts.append(panel)
        panel.makeKeyAndOrderFront(nil)
        return child
    }

    func webViewDidClose(_ view: WKWebView) {
        if view !== web { view.window?.close() }
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        let origin = message.frameInfo.securityOrigin
        guard message.frameInfo.isMainFrame, origin.protocol == "http", origin.host == "127.0.0.1",
              Int(port ?? 0) == origin.port,
              let body = message.body as? [String: Any], let type = body["type"] as? String
        else { return }
        guard message.webView === web else { return }
        switch type {
        case "badge":
            guard let count = body["count"] as? Int else { return }
            NSApp.dockTile.badgeLabel = count > 0 ? String(count) : nil
        case "notify":
            // Fire when the main window is not visibly frontmost (hidden via ⌘W still leaves the app "active").
            let front = NSApp.isActive && window.isVisible && NSApp.keyWindow === window
            guard !front, let id = body["id"] as? String, let title = body["title"] as? String,
                  let text = body["body"] as? String else { return }
            let center = UNUserNotificationCenter.current()
            Task {
                guard (try? await center.requestAuthorization(options: [.alert, .sound])) == true else { return }
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = text
                content.userInfo = ["id": id]
                try? await center.add(UNNotificationRequest(identifier: id, content: content, trigger: nil))
            }
        case "ready":
            window.title = appName
        default: break
        }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse, withCompletionHandler completion: @escaping () -> Void) {
        let id = response.notification.request.content.userInfo["id"] as? String
        Task { @MainActor in
            defer { completion() }
            guard let id else { return }
            showWindow()
            _ = try? await web.callAsyncJavaScript(
                "window.dispatchEvent(new CustomEvent('syncaidync:open', { detail: { id } }));",
                arguments: ["id": id], in: nil, contentWorld: .page)
        }
    }

    private func showWindow() {
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === window else { return true }
        sender.orderOut(nil)
        return false
    }

    func windowWillClose(_ notification: Notification) {
        if let closed = notification.object as? NSWindow {
            popouts.removeAll { $0 === closed }
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showWindow()
        return true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if terminating { return .terminateNow }
        if quitPending { return .terminateLater }
        quitPending = true
        Task {
            let result = try? await web.evaluateJavaScript("window.__syncaidyncLiveCount?.() ?? null")
            if server?.isRunning == true && (result as? Int ?? 1) > 0 {
                let alert = NSAlert()
                alert.messageText = "Quit SyncAiDync?"
                alert.informativeText = "Running sessions will be stopped."
                alert.addButton(withTitle: "Quit")
                alert.addButton(withTitle: "Cancel")
                if alert.runModal() != .alertFirstButtonReturn {
                    quitPending = false
                    sender.reply(toApplicationShouldTerminate: false)
                    return
                }
            }
            terminating = true
            await stopServer()
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    private func registerHotkey() {
        var event = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, pointer in
            guard let pointer else { return OSStatus(eventNotHandledErr) }
            let app = Unmanaged<App>.fromOpaque(pointer).takeUnretainedValue()
            DispatchQueue.main.async { app.toggle() }
            return noErr
        }, 1, &event, Unmanaged.passUnretained(self).toOpaque(), &hotkeyHandler)
        let identifier = EventHotKeyID(signature: 0x44594e43, id: 1)
        RegisterEventHotKey(UInt32(kVK_ANSI_S), UInt32(controlKey | optionKey | cmdKey), identifier,
                            GetApplicationEventTarget(), 0, &hotkey)
    }

    private func toggle() {
        if NSApp.isActive && window.isVisible { NSApp.hide(nil) } else { showWindow() }
    }

    private var currentWeb: WKWebView {
        NSApp.keyWindow?.contentView?.subviews.compactMap { $0 as? WKWebView }.first ?? web
    }

    @objc private func reload() { currentWeb.reload() }
    @objc private func actualSize() { currentWeb.pageZoom = 1 }
    @objc private func zoomIn() { currentWeb.pageZoom = min(currentWeb.pageZoom * 1.1, 3) }
    @objc private func zoomOut() { currentWeb.pageZoom = max(currentWeb.pageZoom / 1.1, 0.4) }

    private func installMenus() {
        let main = NSMenu()
        func menu(_ name: String) -> NSMenu {
            let item = NSMenuItem()
            let result = NSMenu(title: name)
            item.submenu = result
            main.addItem(item)
            return result
        }
        @discardableResult
        func add(_ menu: NSMenu, _ title: String, _ action: Selector, _ key: String = "",
                 target: AnyObject? = nil, modifiers: NSEvent.ModifierFlags = .command) -> NSMenuItem {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
            item.target = target
            item.keyEquivalentModifierMask = modifiers
            menu.addItem(item)
            return item
        }
        let app = menu(appName)
        add(app, "About SyncAiDync", #selector(NSApplication.orderFrontStandardAboutPanel(_:)))
        app.addItem(.separator())
        add(app, "Hide SyncAiDync", #selector(NSApplication.hide(_:)), "h")
        add(app, "Quit SyncAiDync", #selector(NSApplication.terminate(_:)), "q")
        let edit = menu("Edit")
        add(edit, "Undo", Selector(("undo:")), "z")
        add(edit, "Redo", Selector(("redo:")), "z", modifiers: [.command, .shift])
        edit.addItem(.separator())
        add(edit, "Cut", #selector(NSText.cut(_:)), "x")
        add(edit, "Copy", #selector(NSText.copy(_:)), "c")
        add(edit, "Paste", #selector(NSText.paste(_:)), "v")
        add(edit, "Select All", #selector(NSText.selectAll(_:)), "a")
        let view = menu("View")
        add(view, "Reload", #selector(reload), "r", target: self)
        add(view, "Actual Size", #selector(actualSize), "0", target: self)
        add(view, "Zoom In", #selector(zoomIn), "=", target: self)
        add(view, "Zoom Out", #selector(zoomOut), "-", target: self)
        let windows = menu("Window")
        add(windows, "Minimize", #selector(NSWindow.performMiniaturize(_:)), "m")
        add(windows, "Zoom", #selector(NSWindow.performZoom(_:)))
        add(windows, "Bring All to Front", #selector(NSApplication.arrangeInFront(_:)))
        NSApp.windowsMenu = windows
        NSApp.mainMenu = main
    }
}

@main
private enum SyncAiDync {
    @MainActor static func main() {
        let app = NSApplication.shared
        let delegate = App()
        app.setActivationPolicy(.regular)
        app.delegate = delegate
        app.run()
    }
}
