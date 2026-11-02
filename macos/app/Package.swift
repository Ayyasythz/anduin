// swift-tools-version:5.9
// Native window for Anduin: a small AppKit terminal (SwiftTerm) that runs
// anduin-cli, so Anduin gets its own Dock icon and Cmd+Tab entry.
import PackageDescription

let package = Package(
    name: "Anduin",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", from: "1.2.0"),
    ],
    targets: [
        .executableTarget(
            name: "Anduin",
            dependencies: [.product(name: "SwiftTerm", package: "SwiftTerm")]
        ),
    ]
)
