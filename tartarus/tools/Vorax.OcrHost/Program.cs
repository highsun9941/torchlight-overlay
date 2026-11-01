using System.Buffers.Binary;
using System.Diagnostics;
using System.Text.Json;
using Tartarus.Vision;
using Tartarus.Vision.Ocr;

string? Opt(string name) { int at = Array.IndexOf(args, name); return at >= 0 && at + 1 < args.Length ? args[at + 1] : null; }
int threads = int.TryParse(Opt("--threads"), out var n) ? n : 1;
Stream input = Console.OpenStandardInput(), output = Console.OpenStandardOutput();
void Reply(object value)
{
    byte[] bytes = JsonSerializer.SerializeToUtf8Bytes(value);
    Span<byte> length = stackalloc byte[4]; BinaryPrimitives.WriteInt32LittleEndian(length, bytes.Length);
    output.Write(length); output.Write(bytes); output.Flush();
}
try
{
    var startup = Stopwatch.StartNew();
    using var reader = new VoraxTextReader(Opt("--models"), threads);
    if (Opt("--benchmark") is { } folder)
    {
        var records = new List<object>();
        foreach (string path in Directory.GetFiles(folder, "*.png").Order(StringComparer.Ordinal))
        {
            var frame = ImageIO.Load(path);
            var readings = new List<VoraxTextReader.Reading>();
            for (int i = 0; i < 6; i++) readings.Add(reader.Read(frame));
            records.Add(new { file = Path.GetFileName(path), text = readings[^1].Text, readings });
        }
        Console.WriteLine(JsonSerializer.Serialize(new { backend = VoraxTextReader.Backend, threads = reader.Threads,
            inputHeight = reader.Height, workingSetBytes = Process.GetCurrentProcess().WorkingSet64, records }));
        return 0;
    }
    if (!args.Contains("--serve")) throw new ArgumentException("Expected --serve or --benchmark <crop directory>.");
    Reply(new { ready = true, backend = VoraxTextReader.Backend, language = "ko-KR", threads = reader.Threads,
        modelSha256 = reader.ModelSha256, inputHeight = reader.Height, startupMs = startup.Elapsed.TotalMilliseconds });
    byte[] header = new byte[12];
    while (true)
    {
        int first = input.ReadByte();
        if (first < 0) return 0; // Parent closed the pipe; do not leave a worker behind.
        header[0] = (byte)first; input.ReadExactly(header.AsSpan(1));
        int width = BinaryPrimitives.ReadInt32LittleEndian(header.AsSpan(0, 4));
        int height = BinaryPrimitives.ReadInt32LittleEndian(header.AsSpan(4, 4));
        int length = BinaryPrimitives.ReadInt32LittleEndian(header.AsSpan(8, 4));
        if (width <= 0 || height <= 0 || width > 4096 || height > 4096 || length <= 0 || length > 16 * 1024 * 1024 || (long)width * height * 4 != length)
            throw new InvalidDataException("Invalid OCR frame size.");
        byte[] pixels = new byte[length]; input.ReadExactly(pixels);
        var result = reader.Read(new Frame(width, height, pixels, PixelFormat.Rgba32));
        Reply(new { text = result.Text, lines = result.Lines, segmentMs = result.SegmentMs, inferenceMs = result.InferenceMs,
            workingSetBytes = Process.GetCurrentProcess().WorkingSet64 });
    }
}
catch (Exception ex)
{
    Reply(new { error = ex.Message });
    return 1;
}
