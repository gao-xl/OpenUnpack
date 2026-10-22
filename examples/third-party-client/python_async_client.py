#!/usr/bin/env python3
"""
OpenUnpack Async Python Client Example
Demonstrates full automated lifecycle:
1. Connecting to Local REST API
2. Listening to real-time WebSocket progress and password requests
3. Submitting an archive for background extraction
4. Interactively replying with password when requested
"""

import asyncio
import json
import os
import sys
import aiohttp

DEFAULT_URL = os.environ.get("OPENUNPACK_URL", "http://127.0.0.1:18420")
DEFAULT_TOKEN = os.environ.get("OPENUNPACK_TOKEN", "")

class OpenUnpackClient:
    def __init__(self, base_url: str = DEFAULT_URL, token: str = DEFAULT_TOKEN):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.headers = {"Content-Type": "application/json"}
        if token:
            self.headers["Authorization"] = f"Bearer {token}"

    async def send_command(self, session: aiohttp.ClientSession, command: str, args: dict = None) -> dict:
        url = f"{self.base_url}/api/v1/commands"
        payload = {"command": command}
        if args is not None:
            payload["args"] = args
        async with session.post(url, headers=self.headers, json=payload) as resp:
            data = await resp.json()
            if resp.status >= 400:
                raise RuntimeError(f"Command '{command}' failed: {data}")
            return data.get("data")

    async def get_status(self, session: aiohttp.ClientSession) -> dict:
        async with session.get(f"{self.base_url}/api/v1/status", headers=self.headers) as resp:
            return await resp.json()

    async def add_archive(self, session: aiohttp.ClientSession, path: str, output_dir: str = None, password: str = None) -> list:
        args = {
            "path": path,
            "outputDir": output_dir,
            "password": password,
            "autoStart": True
        }
        return await self.send_command(session, "AddArchive", args)

    async def submit_password(self, session: aiohttp.ClientSession, job_id: str, password: str) -> dict:
        args = {
            "jobId": job_id,
            "inputType": "password",
            "value": password
        }
        return await self.send_command(session, "SubmitInput", args)

    async def listen_events(self, on_progress=None, on_need_password=None):
        ws_url = self.base_url.replace("http://", "ws://").replace("https://", "wss://") + "/api/v1/ws"
        if self.token:
            ws_url += f"?token={self.token}"

        async with aiohttp.ClientSession() as session:
            async with session.ws_connect(ws_url) as ws:
                print(f"[WebSocket] Connected to {ws_url}")
                async for msg in ws:
                    if msg.type == aiohttp.WSMsgType.TEXT:
                        ev = json.loads(msg.data)
                        kind = ev.get("event")
                        data = ev.get("data")
                        if kind == "job_progress" and on_progress:
                            await on_progress(data)
                        elif kind == "need_password" and on_need_password:
                            await on_need_password(session, data)
                    elif msg.type in (aiohttp.WSMsgType.CLOSED, aiohttp.WSMsgType.ERROR):
                        break

async def main():
    if len(sys.argv) < 2:
        print("Usage: python python_async_client.py <ARCHIVE_PATH> [OUTPUT_DIR]")
        sys.exit(1)

    archive_path = sys.argv[1]
    output_dir = sys.argv[2] if len(sys.argv) > 2 else None

    client = OpenUnpackClient()

    async def handle_progress(update):
        job_id = update.get("jobId")
        bytes_done = update.get("bytesExtracted", 0)
        total_bytes = update.get("totalBytes", 0)
        current = update.get("currentFile", "")
        pct = (bytes_done / total_bytes * 100) if total_bytes > 0 else 0
        print(f"[Progress] Job {job_id[:8]} -> {pct:.1f}% ({bytes_done}/{total_bytes} bytes) | {current}")

    async def handle_need_password(session, event):
        job_id = event.get("jobId")
        archive = event.get("archivePath", "")
        print(f"\n[ALERT] Archive requires password: {archive}")
        pwd = input(f"Enter password for job {job_id}: ").strip()
        await client.submit_password(session, job_id, pwd)
        print("[Submit] Password submitted to daemon.")

    async with aiohttp.ClientSession() as session:
        print("[HTTP] Fetching service status...")
        try:
            status = await client.get_status(session)
            print(f"[HTTP] OpenUnpack Daemon Status: {status}")
        except Exception as e:
            print(f"[Error] Failed to connect to OpenUnpack Daemon: {e}")
            sys.exit(1)

        print(f"[HTTP] Submitting archive: {archive_path}")
        jobs = await client.add_archive(session, archive_path, output_dir)
        print(f"[HTTP] Jobs queued: {jobs}")

    print("[Stream] Listening to real-time events...")
    await client.listen_events(on_progress=handle_progress, on_need_password=handle_need_password)

if __name__ == "__main__":
    asyncio.run(main())
