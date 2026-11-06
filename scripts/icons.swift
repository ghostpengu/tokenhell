// Generates tray template icons and the app icon source. Run: swift scripts/icons.swift
import AppKit

func flame(_ s: CGFloat, inner: Bool) -> NSBezierPath {
    // Designed in a 100x100 box, y down; flipped below.
    func p(_ x: CGFloat, _ y: CGFloat) -> NSPoint { NSPoint(x: x * s / 100, y: (100 - y) * s / 100) }
    let o = NSBezierPath()
    o.move(to: p(50, 96))
    o.curve(to: p(17, 63), controlPoint1: p(29, 96), controlPoint2: p(15, 82))
    o.curve(to: p(38, 24), controlPoint1: p(19, 46), controlPoint2: p(35, 39))
    o.curve(to: p(37, 3), controlPoint1: p(40, 14), controlPoint2: p(39, 7))
    o.curve(to: p(61, 41), controlPoint1: p(54, 10), controlPoint2: p(64, 25))
    o.curve(to: p(69, 25), controlPoint1: p(67, 37), controlPoint2: p(69, 31))
    o.curve(to: p(83, 66), controlPoint1: p(81, 38), controlPoint2: p(86, 52))
    o.curve(to: p(50, 96), controlPoint1: p(80, 84), controlPoint2: p(67, 96))
    o.close()
    if inner {
        let i = NSBezierPath()
        i.move(to: p(50, 87))
        i.curve(to: p(36, 71), controlPoint1: p(41, 87), controlPoint2: p(34, 80))
        i.curve(to: p(48, 50), controlPoint1: p(38, 62), controlPoint2: p(47, 58))
        i.curve(to: p(63, 72), controlPoint1: p(57, 56), controlPoint2: p(63, 64))
        i.curve(to: p(50, 87), controlPoint1: p(63, 81), controlPoint2: p(58, 87))
        i.close()
        o.append(i)
        o.windingRule = .evenOdd
    }
    return o
}

func png(_ size: Int, _ path: String, draw: (CGFloat) -> Void) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    draw(CGFloat(size))
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
}

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "src-tauri/icons"

// Tray: 44px (22pt @2x), black on transparent: macOS recolours template images.
for (name, inner) in [("tray.png", true), ("tray-hot.png", false)] {
    png(44, "\(out)/\(name)") { s in
        let inset: CGFloat = 4
        let t = NSAffineTransform(); t.translateX(by: inset, yBy: inset)
        let f = flame(s - inset * 2, inner: inner); f.transform(using: t as AffineTransform)
        NSColor.black.setFill(); f.fill()
    }
}

// App icon source: dark glassy squircle with a warm flame.
png(1024, "\(out)/app-icon.png") { s in
    let r = NSRect(x: 100, y: 100, width: s - 200, height: s - 200)
    let sq = NSBezierPath(roundedRect: r, xRadius: 185, yRadius: 185)
    NSGradient(colors: [NSColor(calibratedRed: 0.16, green: 0.10, blue: 0.12, alpha: 1),
                        NSColor(calibratedRed: 0.05, green: 0.04, blue: 0.06, alpha: 1)])!.draw(in: sq, angle: -90)
    NSColor(white: 1, alpha: 0.10).setStroke(); sq.lineWidth = 6; sq.stroke()
    let fs: CGFloat = 520
    let t = NSAffineTransform(); t.translateX(by: (s - fs) / 2, yBy: (s - fs) / 2 - 10)
    let f = flame(fs, inner: false); f.transform(using: t as AffineTransform)
    NSGradient(colors: [NSColor(calibratedRed: 1.0, green: 0.78, blue: 0.30, alpha: 1),
                        NSColor(calibratedRed: 1.0, green: 0.36, blue: 0.16, alpha: 1),
                        NSColor(calibratedRed: 0.86, green: 0.10, blue: 0.22, alpha: 1)])!.draw(in: f, angle: 90 + 180)
    let i = flame(fs, inner: true); i.transform(using: t as AffineTransform)
    NSColor(white: 1, alpha: 0.0).setFill()
}
print("icons written to \(out)")
