using System.Diagnostics;
using System.Security.Cryptography;
using Microsoft.ML.OnnxRuntime;

namespace Tartarus.Vision.Ocr;

/// <summary>CPU-only recognition of the already cropped Vorax text, without a full-screen detector.</summary>
public sealed class VoraxTextReader : IDisposable
{
    public const string Backend = "paddleocr-v5-korean-cpu";
    private readonly SessionOptions _options;
    private readonly TextRecognizer _recognizer;
    public int Threads { get; }
    public int Height => _recognizer.Height;
    public string ModelSha256 { get; }

    public VoraxTextReader(string? modelDir = null, int threads = 1)
    {
        modelDir ??= OcrEngine.FindModelDir();
        var pack = OcrPacks.Korean;
        string model = Path.Combine(modelDir, pack.Model), dict = Path.Combine(modelDir, pack.Dictionary);
        Check(model, pack.ModelSize, pack.ModelSha256);
        Check(dict, pack.DictionarySize, pack.DictionarySha256);
        ModelSha256 = pack.ModelSha256;
        Threads = Math.Clamp(threads, 1, 4);
        _options = new SessionOptions { IntraOpNumThreads = Threads, InterOpNumThreads = 1,
            ExecutionMode = ExecutionMode.ORT_SEQUENTIAL, GraphOptimizationLevel = GraphOptimizationLevel.ORT_ENABLE_ALL };
        _options.AddSessionConfigEntry("session.intra_op.allow_spinning", "0");
        _options.AddSessionConfigEntry("session.inter_op.allow_spinning", "0");
        try { _recognizer = new TextRecognizer(model, dict, _options) { BatchSize = 1, MaxWidth = 1024 }; }
        catch { _options.Dispose(); throw; }
    }

    private static void Check(string path, long size, string expected)
    {
        if (!File.Exists(path) || new FileInfo(path).Length != size) throw new InvalidDataException($"OCR model file missing or wrong size: {Path.GetFileName(path)}");
        using var stream = File.OpenRead(path);
        if (!Convert.ToHexString(SHA256.HashData(stream)).Equals(expected, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException($"OCR model hash mismatch: {Path.GetFileName(path)}");
    }

    public sealed record Line(string Text, float Confidence, PixelBox Box);
    public sealed record Reading(string Text, IReadOnlyList<Line> Lines, double SegmentMs, double InferenceMs);

    public Reading Read(Frame frame)
    {
        var sw = Stopwatch.StartNew();
        var boxes = TextRows(frame);
        double segmentMs = sw.Elapsed.TotalMilliseconds;
        TextRecognizer.Result[] results = boxes.Count == 0 ? [] : _recognizer.Recognize(frame, boxes);
        double inferenceMs = sw.Elapsed.TotalMilliseconds - segmentMs;
        var lines = results.Select((r, i) => new Line(r.Text, r.Conf, boxes[i]))
            .Where(l => l.Text.Length > 0 && l.Confidence >= 0.55f).ToArray();
        return new Reading(string.Join('\n', lines.Select(l => l.Text)), lines, segmentMs, inferenceMs);
    }

    /// <summary>Find text rows in dark-on-white OCR inputs, excluding long vertical card rims.</summary>
    public static List<PixelBox> TextRows(Frame frame)
    {
        int w = frame.Width, h = frame.Height;
        var ink = new bool[w * h];
        var columns = new int[w];
        for (int y = 0; y < h; y++) for (int x = 0; x < w; x++)
        {
            int p = (y * w + x) * 4;
            bool dark = Math.Max(frame.Pixels[p], Math.Max(frame.Pixels[p + 1], frame.Pixels[p + 2])) < 180;
            ink[y * w + x] = dark;
            if (dark) columns[x]++;
        }
        // These are borders, not letter stems; a padded heading cannot have a full-height stem.
        var rim = columns.Select(n => n > h * 0.85).ToArray();
        var runs = new List<(int Top, int Bottom)>();
        int start = -1, last = -1;
        for (int y = 0; y < h; y++)
        {
            int hits = 0;
            for (int x = w / 5; x < w * 4 / 5; x++) if (!rim[x] && ink[y * w + x]) hits++;
            if (hits < Math.Max(3, w / 100)) continue;
            if (last < 0 || y - last > 3) { if (last >= 0) runs.Add((start, last + 1)); start = y; }
            last = y;
        }
        if (last >= 0) runs.Add((start, last + 1));
        var boxes = new List<PixelBox>();
        foreach (var (top, bottom) in runs)
        {
            if (bottom - top < 4 || bottom - top > 128) continue;
            int left = w, right = -1, pixels = 0;
            for (int x = 0; x < w; x++)
            {
                if (rim[x]) continue;
                int count = 0;
                for (int y = top; y < bottom; y++) if (ink[y * w + x]) count++;
                if (count < 2) continue;
                left = Math.Min(left, x); right = x; pixels += count;
            }
            if (right < left || right - left < 3) continue;
            // A bold, isolated digit can fill slightly over 80% of its tight bounds (the
            // reported activity '5' fills 80.58%). Keep it; solid artwork still fails the
            // density check, and wide text retains the stricter original threshold.
            double densityLimit = right - left + 1 <= (bottom - top) * 1.5 ? 0.92 : 0.8;
            if (pixels > (right - left + 1) * (bottom - top) * densityLimit) continue;
            int pad = Math.Max(2, (bottom - top) / 12);
            boxes.Add(new PixelBox(left - pad, top - pad, right + 1 + pad, bottom + pad).Clamp(w, h));
            if (boxes.Count == 12) break;
        }
        return boxes;
    }

    public void Dispose() { _recognizer.Dispose(); _options.Dispose(); }
}
