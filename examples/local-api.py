"""Python 3 standard-library client; token comes from process environment.

Usage: python local-api.py --archive C:\\Input\\example.zip --output C:\\Output
Without --archive, prints the authorized queue snapshot.
"""
import argparse
import json
import os
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=47980)
    parser.add_argument("--archive")
    parser.add_argument("--output")
    args = parser.parse_args()
    token = os.environ.get("OPENUNPACK_API_TOKEN")
    if not token:
        parser.error("Set OPENUNPACK_API_TOKEN in this process before calling.")
    if not 1 <= args.port <= 65535:
        parser.error("Invalid port.")
    if args.archive and not args.output:
        parser.error("--archive requires --output.")

    def command(name, arguments=None):
        body = {"command": name}
        if arguments is not None:
            body["args"] = arguments
        request = urllib.request.Request(
            f"http://127.0.0.1:{args.port}/api/v1/commands",
            data=json.dumps(body).encode("utf-8"),
            headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        )
        # Explicitly bypass proxy settings for the local service.
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        try:
            with opener.open(request, timeout=120) as response:
                return json.load(response)["data"]
        except urllib.error.HTTPError as error:
            result = json.load(error)
            raise RuntimeError(f"HTTP {error.code}: {result['error']['message']} (request {result['requestId']})") from None

    if args.archive:
        scan = command("scan_sources", {"paths": [os.path.abspath(args.archive)]})
        settings = command("read_settings")
        settings["outputRoot"] = os.path.abspath(args.output)
        settings["sourceCleanup"] = "keep"
        command("add_jobs", {"sources": scan["sources"], "options": settings})
        command("start_jobs")
    print(json.dumps(command("queue_snapshot"), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
