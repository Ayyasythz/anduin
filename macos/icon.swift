// Draws the 1024x1024 app icon: the graduation kitten (the same character as
// the in-app mascot, drawn at a higher resolution) on a blue rounded square.
// Usage: swift icon.swift out.png
import AppKit

let size = 1024

// 24x20 pixel art. '.' is transparent.
let art = [
    "........................",
    "...........KK...........",
    "........KKKKKKKK........",
    ".....KKKKKKKKKKKKKK.....",
    "..KKKKKKKKKYYKKKKKKKKK..",
    ".YKKKKKKKKKKKKKKKKKKKKK.",
    ".Y..KKKKKKKKKKKKKKKK....",
    ".Y.OO..DDDDDDDDDD..OO...",
    ".Y.OPOODDDDDDDDDDOOPO...",
    "YY.OOOOOOLOLLOLOOOOOO...",
    "YYOOOOOOOOOOOOOOOOOOOO..",
    "YYOOOWEEOOOOOOOOWEEOOO..",
    "YYOOOEEEOOOOOOOOEEEOOO..",
    ".LOOOOOOOWWWWWWOOOOOOOL.",
    ".LLOOOOOWWWPPWWWOOOOOLL.",
    ".OOOOOOWWWWMMWWWWOOOOOO.",
    "..OOOOOWWWMWWMWWWOOOOO..",
    "...OOOOOWWWWWWWWOOOOO...",
    ".....OOOOOOOOOOOOOO.....",
    "........................",
]
let palette: [Character: NSColor] = [
    "K": rgb(0x464858), "D": rgb(0x2c2e3a), "Y": rgb(0xecc05c), "O": rgb(0xeca052),
    "L": rgb(0xb8702e), "E": rgb(0x121216), "W": rgb(0xf6eee4), "P": rgb(0xf096a5),
    "M": rgb(0xc77a74),
]

func rgb(_ hex: Int) -> NSColor {
    NSColor(srgbRed: CGFloat(hex >> 16 & 0xff) / 255, green: CGFloat(hex >> 8 & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: 1)
}

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSGraphicsContext.current!.imageInterpolation = .none

// Body, inset like the standard macOS icon grid.
let body = NSBezierPath(roundedRect: NSRect(x: 100, y: 100, width: 824, height: 824),
                        xRadius: 185, yRadius: 185)
NSGradient(starting: rgb(0x6a9cf0), ending: rgb(0x2c4f9e))!.draw(in: body, angle: -90)
NSColor(white: 1, alpha: 0.12).setStroke()
body.lineWidth = 6
body.stroke()

// The kitten, centered, with a soft drop shadow.
let px: CGFloat = 28
let cols = CGFloat(art[0].count), rows = CGFloat(art.count)
let originX = (CGFloat(size) - cols * px) / 2
let originY = (CGFloat(size) + rows * px) / 2 - 10

let shadow = NSShadow()
shadow.shadowColor = NSColor(white: 0, alpha: 0.35)
shadow.shadowOffset = NSSize(width: 0, height: -14)
shadow.shadowBlurRadius = 24
shadow.set()
let cg = NSGraphicsContext.current!.cgContext
cg.beginTransparencyLayer(auxiliaryInfo: nil)

for (r, line) in art.enumerated() {
    for (c, ch) in line.enumerated() {
        guard let color = palette[ch] else { continue }
        color.setFill()
        // Overlap by a hair so no seams show between pixels.
        NSRect(x: originX + CGFloat(c) * px, y: originY - CGFloat(r + 1) * px,
               width: px + 0.5, height: px + 0.5).fill()
    }
}

cg.endTransparencyLayer()

NSGraphicsContext.restoreGraphicsState()
let png = rep.representation(using: .png, properties: [:])!
try! png.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
