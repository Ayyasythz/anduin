// Anduin.app: a native window that runs anduin-cli in a terminal view, so the
// app has its own Dock icon, Cmd+Tab entry and menu bar.
import AppKit
import SwiftTerm

let background = NSColor(srgbRed: 0.059, green: 0.063, blue: 0.078, alpha: 1)
let foreground = NSColor(srgbRed: 0.86, green: 0.87, blue: 0.89, alpha: 1)
let accent = NSColor(srgbRed: 0.85, green: 0.47, blue: 0.34, alpha: 1)  // Claude orange
let padding = NSSize(width: 8, height: 6)
let fontSizeKey = "fontSize"

final class AnduinTerminalView: LocalProcessTerminalView {
    /// Bounce the Dock icon when a session rings and Anduin is in the background.
    nonisolated override func bell(source: Terminal) {
        super.bell(source: source)
        DispatchQueue.main.async {
            if !NSApp.isActive {
                NSApp.requestUserAttention(.informationalRequest)
            }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate,
    LocalProcessTerminalViewDelegate
{
    var window: NSWindow!
    var term: AnduinTerminalView!
    var running = false

    func applicationDidFinishLaunching(_ note: Notification) {
        NSApp.mainMenu = buildMenu()

        let size = UserDefaults.standard.double(forKey: fontSizeKey)
        let font = NSFont.monospacedSystemFont(ofSize: size > 0 ? size : 13, weight: .regular)

        term = AnduinTerminalView(frame: .zero)
        term.font = font
        term.nativeBackgroundColor = background
        term.nativeForegroundColor = foreground
        term.caretColor = accent
        term.optionAsMetaKey = false
        term.processDelegate = self

        // Open at 170x42 cells (what the Terminal.app launcher used), or the
        // size the user last left the window at.
        let cell = cellSize(font)
        let content = NSSize(
            width: cell.width * 170 + padding.width * 2,
            height: cell.height * 42 + padding.height * 2)
        window = NSWindow(
            contentRect: NSRect(origin: .zero, size: content),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered, defer: false)
        window.title = "Anduin"
        window.appearance = NSAppearance(named: .darkAqua)
        window.backgroundColor = background
        window.titlebarAppearsTransparent = true
        window.collectionBehavior.insert(.fullScreenPrimary)
        window.minSize = NSSize(width: 480, height: 240)
        window.delegate = self

        let container = NSView(frame: NSRect(origin: .zero, size: content))
        container.wantsLayer = true
        container.layer?.backgroundColor = background.cgColor
        term.frame = container.bounds.insetBy(dx: padding.width, dy: padding.height)
        term.autoresizingMask = [.width, .height]
        container.addSubview(term)
        window.contentView = container

        window.center()
        window.setFrameAutosaveName("AnduinMain")
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(term)
        NSApp.activate(ignoringOtherApps: true)

        start()
    }

    func start() {
        let cli = Bundle.main.executableURL!.deletingLastPathComponent()
            .appendingPathComponent("anduin-cli").path
        let shell = loginShell()
        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["TERM_PROGRAM"] = "Anduin"
        if env["LANG"] == nil { env["LANG"] = "en_US.UTF-8" }
        for key in ["__CFBundleIdentifier", "XPC_SERVICE_NAME", "XPC_FLAGS", "CLAUDECODE"] {
            env.removeValue(forKey: key)
        }
        // When launched from inside a Claude Code session, don't make the
        // sessions in Anduin look like its children.
        for key in env.keys where key.hasPrefix("CLAUDE_CODE_") {
            env.removeValue(forKey: key)
        }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        // A login + interactive shell picks up PATH from ~/.zprofile and
        // ~/.zshrc (where `claude` lives), like Terminal.app does. Extra
        // arguments given to the app are passed on to anduin.
        let args = Array(CommandLine.arguments.dropFirst()).filter { !$0.hasPrefix("-psn_") }
        term.startProcess(
            executable: shell,
            args: ["-l", "-i", "-c", "exec \"$0\" \"$@\"", cli] + args,
            environment: env.map { "\($0.key)=\($0.value)" },
            execName: "-" + (shell as NSString).lastPathComponent,
            currentDirectory: home)
        running = true
    }

    // MARK: LocalProcessTerminalViewDelegate

    func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

    func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
        window.title = title.isEmpty ? "Anduin" : title
    }

    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

    func processTerminated(source: TerminalView, exitCode: Int32?) {
        running = false
        NSApp.terminate(nil)
    }

    // MARK: Quitting

    func applicationShouldTerminateAfterLastWindowClosed(_ app: NSApplication) -> Bool { true }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        confirmQuit()
    }

    func applicationShouldTerminate(_ app: NSApplication) -> NSApplication.TerminateReply {
        confirmQuit() ? .terminateNow : .terminateCancel
    }

    func applicationWillTerminate(_ note: Notification) {
        if running {
            running = false
            term.terminate()
        }
    }

    private var confirmed = false

    /// Asks before stopping anduin (and the Claude sessions inside it).
    private func confirmQuit() -> Bool {
        if !running || confirmed { return true }
        let alert = NSAlert()
        alert.messageText = "Quit Anduin?"
        alert.informativeText = "All Claude sessions running in Anduin will be stopped."
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        confirmed = alert.runModal() == .alertFirstButtonReturn
        return confirmed
    }

    // MARK: Font size

    @objc func biggerFont(_ sender: Any?) { setFontSize(term.font.pointSize + 1) }
    @objc func smallerFont(_ sender: Any?) { setFontSize(term.font.pointSize - 1) }
    @objc func resetFont(_ sender: Any?) { setFontSize(13) }

    private func setFontSize(_ size: CGFloat) {
        let size = min(max(size, 8), 32)
        term.font = NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
        UserDefaults.standard.set(Double(size), forKey: fontSizeKey)
    }

    // MARK: Helpers

    private func cellSize(_ font: NSFont) -> NSSize {
        let w = ("W" as NSString).size(withAttributes: [.font: font]).width
        let h = ceil(font.ascender - font.descender + font.leading)
        return NSSize(width: ceil(w), height: h)
    }

    private func loginShell() -> String {
        if let pw = getpwuid(getuid()), let sh = pw.pointee.pw_shell {
            let s = String(cString: sh)
            if FileManager.default.isExecutableFile(atPath: s) { return s }
        }
        return "/bin/zsh"
    }

    private func buildMenu() -> NSMenu {
        let main = NSMenu()

        let app = NSMenu()
        app.addItem(withTitle: "About Anduin",
                    action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Hide Anduin", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let others = app.addItem(withTitle: "Hide Others",
                                 action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        others.keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Quit Anduin", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        main.addItem(withTitle: "Anduin", action: nil, keyEquivalent: "").submenu = app

        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        main.addItem(withTitle: "Edit", action: nil, keyEquivalent: "").submenu = edit

        let view = NSMenu(title: "View")
        view.addItem(withTitle: "Bigger", action: #selector(biggerFont(_:)), keyEquivalent: "+")
        view.addItem(withTitle: "Bigger", action: #selector(biggerFont(_:)), keyEquivalent: "=")
            .isHidden = true
        view.addItem(withTitle: "Smaller", action: #selector(smallerFont(_:)), keyEquivalent: "-")
        view.addItem(withTitle: "Actual Size", action: #selector(resetFont(_:)), keyEquivalent: "0")
        view.addItem(.separator())
        let full = view.addItem(withTitle: "Enter Full Screen",
                                action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
        full.keyEquivalentModifierMask = [.command, .control]
        main.addItem(withTitle: "View", action: nil, keyEquivalent: "").submenu = view

        let win = NSMenu(title: "Window")
        win.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        win.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        win.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        main.addItem(withTitle: "Window", action: nil, keyEquivalent: "").submenu = win
        NSApp.windowsMenu = win

        return main
    }
}

MainActor.assumeIsolated {
    let app = NSApplication.shared
    let delegate = AppDelegate()
    app.delegate = delegate
    app.setActivationPolicy(.regular)
    app.run()
}
