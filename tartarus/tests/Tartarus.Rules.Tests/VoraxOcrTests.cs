using Tartarus.Vision;
using Tartarus.Vision.Ocr;

namespace Tartarus.Rules.Tests;

public sealed class VoraxOcrTests
{
    private static Frame Canvas()
    {
        var pixels = Enumerable.Repeat((byte)255, 400 * 160 * 4).ToArray();
        return new Frame(400, 160, pixels, PixelFormat.Rgba32);
    }
    private static void Fill(Frame frame, int x, int y, int w, int h)
    {
        for (int yy = y; yy < y + h; yy++) for (int xx = x; xx < x + w; xx++)
            frame.Pixels.AsSpan((yy * frame.Width + xx) * 4, 3).Clear();
    }
    [Fact]
    public void TextRowsIgnoreBlankFramesAndLongCardRimsButKeepSeparateLines()
    {
        var frame = Canvas(); Assert.Empty(VoraxTextReader.TextRows(frame));
        Fill(frame, 35, 0, 3, 160); Fill(frame, 365, 0, 3, 160);
        for (int x = 110; x < 260; x += 25) { Fill(frame, x, 30, 12, 25); Fill(frame, x, 95, 12, 25); }
        var rows = VoraxTextReader.TextRows(frame);
        Assert.Equal(2, rows.Count);
        Assert.All(rows, r => Assert.InRange(r.X0, 100, 120));
        Assert.InRange(rows[0].Y0, 25, 30); Assert.InRange(rows[1].Y0, 90, 95);
    }
    [Fact]
    public void SolidArtworkIsNotATextLine()
    {
        var frame = Canvas(); Fill(frame, 70, 15, 270, 135);
        Assert.Empty(VoraxTextReader.TextRows(frame));
    }
}
