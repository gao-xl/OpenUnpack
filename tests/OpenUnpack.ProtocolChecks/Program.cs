using System.Reflection;
using System.Text.Json;
using OpenUnpack.Worker;

var root = Path.GetFullPath(args[0]);
// Use the actual Worker serializer options, including strict number handling.
var options = (JsonSerializerOptions)typeof(WorkerRequest).Assembly.GetType("OpenUnpack.Worker.Protocol")!
    .GetField("Json", BindingFlags.Static | BindingFlags.Public)!.GetValue(null)!;
using var corpus = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(root, "samples.json")));
var results = new List<object>();
foreach (var sample in corpus.RootElement.EnumerateArray())
{
    var name = sample.GetProperty("name").GetString()!;
    var dto = sample.GetProperty("dto").GetString()!;
    var type = typeof(WorkerRequest).Assembly.GetType("OpenUnpack.Worker." + dto)!;
    bool accepted; JsonElement? output = null;
    try
    {
        var decoded = JsonSerializer.Deserialize(sample.GetProperty("input").GetRawText(), type, options);
        output = JsonSerializer.SerializeToElement(decoded, type, options); accepted = true;
    }
    catch (JsonException) { accepted = false; }
    if (accepted != sample.GetProperty("csharpAccepts").GetBoolean())
        throw new Exception("C# contract expectation failed: " + name);
    results.Add(new { name, accepted, output });
    Console.WriteLine("PASS actual DTO " + name);
}
File.WriteAllBytes(Path.Combine(root,"csharp-results.json"),JsonSerializer.SerializeToUtf8Bytes(results, options));
File.WriteAllText(Path.Combine(root,"compatibility-notes.txt"),
    "Non-null C# reference DTO members can deserialize explicit null or missing values; Rust rejects them. " +
    "WorkerHost/WorkerOperations validate operation-specific input; DTO acceptance is not permission to execute.\n" +
    "WorkerCapabilities.maxBatchCandidates: Rust usize range exceeds C# int; advertised supported batch size is 128.\n" +
    "Numeric strings are rejected in both serializers. Output defaults/null omission are compared through typed Rust decoding.\n");
