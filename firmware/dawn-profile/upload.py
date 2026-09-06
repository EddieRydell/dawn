"""Provision Wi-Fi, upload a prepared sequence over HTTP, and verify frames."""

import argparse
import getpass
import hashlib
import http.client
import logging
import pathlib
import re
import socket
import struct
import subprocess
import sys
import time

import serial


parser = argparse.ArgumentParser()
parser.add_argument("sequence", type=pathlib.Path)
parser.add_argument("--port", default="COM4")
parser.add_argument("--ssid")
parser.add_argument(
    "--windows-profile",
    help="Provision this saved Windows Wi-Fi profile without logging its password",
)
parser.add_argument("--repeat", type=int, default=1)
parser.add_argument("--uploads", type=int, default=1)
parser.add_argument(
    "--exercise-rejections",
    action="store_true",
    help="Test invalid uploads, authorization, and interrupted HTTP bodies",
)
parser.add_argument(
    "--log", type=pathlib.Path, help="New evidence file; existing files are not overwritten"
)
parser.add_argument(
    "--elf",
    type=pathlib.Path,
    default=pathlib.Path(__file__).parent
    / "target/xtensa-esp32-none-elf/release/loader",
)
args = parser.parse_args()

if args.repeat < 1 or args.uploads < 1:
    parser.error("--repeat and --uploads must be positive")
if not args.ssid and not args.windows_profile:
    parser.error("provide --ssid or --windows-profile")

handlers = [logging.StreamHandler(sys.stdout)]
if args.log:
    handlers.append(logging.FileHandler(args.log, mode="x"))
logging.basicConfig(level=logging.INFO, format="%(message)s", handlers=handlers)

payload = args.sequence.read_bytes()
expected = [
    tuple(map(int, line.split()))
    for line in pathlib.Path(str(args.sequence) + ".checksums").read_text().splitlines()
]
logging.info("elf_sha256=%s", hashlib.sha256(args.elf.read_bytes()).hexdigest())
logging.info(
    "payload_bytes=%s payload_sha256=%s transport=http",
    len(payload),
    hashlib.sha256(payload).hexdigest(),
)


def credentials():
    if args.windows_profile:
        result = subprocess.run(
            [
                "netsh",
                "wlan",
                "show",
                "profile",
                f"name={args.windows_profile}",
                "key=clear",
            ],
            capture_output=True,
            check=True,
        )
        profile = result.stdout.decode(errors="replace")
        match = re.search(r"Key Content\s*:\s*(.+)", profile)
        if not match:
            raise RuntimeError("Saved Wi-Fi profile has no accessible personal-network key")
        ssid = (args.ssid or args.windows_profile).encode()
        password = match[1].strip().encode()
        del profile, result, match
    else:
        ssid = args.ssid.encode()
        password = getpass.getpass("Wi-Fi password: ").encode()
    if not 0 < len(ssid) <= 32 or len(password) > 64:
        raise ValueError("Invalid Wi-Fi credential lengths")
    return ssid, password


def provision():
    ssid, password = credentials()
    transcript = []
    with serial.Serial(port=None, baudrate=115200, timeout=0.1) as port:
        port.port = args.port
        port.dtr = False
        port.rts = False
        port.open()
        port.rts = True
        time.sleep(0.1)
        port.reset_input_buffer()
        port.rts = False

        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            port.write(b"P")
            line = port.readline()
            if line:
                transcript.append(line)
                if line == b"DAWN PROVISION READY\n":
                    break
        else:
            recent = b"".join(transcript[-20:])
            raise RuntimeError(f"Provisioning handshake timed out: {recent!r}")

        port.write(b"W" + bytes([len(ssid), len(password)]) + ssid + password)
        del password

        token_line = port.readline().strip()
        if not token_line.startswith(b"TOKEN "):
            raise RuntimeError(f"Device did not return an HTTP token: {token_line!r}")
        token = token_line.removeprefix(b"TOKEN ").decode("ascii")
        if len(token) != 32 or any(character not in "0123456789abcdef" for character in token):
            raise RuntimeError("Device returned an invalid HTTP token")

        deadline = time.monotonic() + 60
        transcript.clear()
        while time.monotonic() < deadline:
            line = port.readline()
            if not line:
                continue
            transcript.append(line)
            text = line.decode("ascii", errors="replace").strip()
            if text.startswith("WIFI READY "):
                _, _, address, http_port, *_ = text.split()
                logging.info(text)
                return address, int(http_port), token
        recent = b"".join(transcript[-20:])
        raise RuntimeError(f"Wi-Fi did not become ready: {recent!r}")


address, port, token = provision()
connection = http.client.HTTPConnection(address, port, timeout=20)


def request(method, path, body, supplied_token=token):
    connection.request(
        method,
        path,
        body=body,
        headers={
            "Content-Type": "application/octet-stream",
            "X-Dawn-Token": supplied_token,
        },
    )
    response = connection.getresponse()
    response_body = response.read().decode("ascii").strip()
    return response.status, response_body


def upload(data, status=200, prefix="LOADED "):
    actual_status, response = request("PUT", "/sequence", data)
    if actual_status != status or not response.startswith(prefix):
        raise RuntimeError(
            f"Expected HTTP {status} and {prefix!r}, received HTTP {actual_status}: {response!r}"
        )
    logging.info(response)


for _ in range(args.uploads):
    start = time.monotonic()
    upload(payload)
    logging.info("transfer_seconds=%.3f", time.monotonic() - start)

if args.exercise_rejections:
    wrong_token = "0" * 32 if token != "0" * 32 else "1" * 32
    status, response = request("PUT", "/sequence", b"", wrong_token)
    assert (status, response) == (401, "Missing or invalid X-Dawn-Token"), (status, response)
    logging.info("VERIFIED HTTP authorization rejection")

    version = bytearray(payload)
    version[4:8] = struct.pack("<I", 999)
    upload(version, 422, "REJECT Version")

    oversized = bytearray(payload[:16])
    oversized[8:12] = struct.pack("<I", 32 * 1024 + 1)
    upload(oversized, 422, "REJECT Limit")

    corrupt = bytearray(payload)
    corrupt[-1] ^= 0x80
    upload(corrupt, 422, "REJECT Checksum")

    connection.close()
    interrupted = socket.create_connection((address, port), timeout=10)
    headers = (
        f"PUT /sequence HTTP/1.1\r\nHost: {address}\r\n"
        f"X-Dawn-Token: {token}\r\nContent-Type: application/octet-stream\r\n"
        f"Content-Length: {len(payload)}\r\nConnection: close\r\n\r\n"
    ).encode("ascii")
    interrupted.sendall(headers + payload[:32])
    time.sleep(0.1)
    concurrent = http.client.HTTPConnection(address, port, timeout=20)
    concurrent.request(
        "PUT",
        "/sequence",
        body=payload,
        headers={
            "Content-Type": "application/octet-stream",
            "X-Dawn-Token": token,
        },
    )
    concurrent_response = concurrent.getresponse()
    concurrent_body = concurrent_response.read().decode("ascii").strip()
    assert (concurrent_response.status, concurrent_body) == (
        409,
        "Another upload is in progress",
    ), (concurrent_response.status, concurrent_body)
    concurrent.close()
    logging.info("VERIFIED concurrent HTTP upload rejection")
    interrupted.close()
    time.sleep(0.2)
    connection = http.client.HTTPConnection(address, port, timeout=20)
    logging.info("VERIFIED interrupted HTTP upload disconnect")

for ticks, checksum in expected * args.repeat:
    status, line = request("POST", "/frame", struct.pack("<I", ticks))
    if status != 200 or not line.startswith("FRAME "):
        raise RuntimeError(f"Frame request failed with HTTP {status}: {line!r}")
    _, actual_ticks, actual_crc, micros, allocations, global_allocations = line.split()
    assert (int(actual_ticks), int(actual_crc), int(allocations)) == (
        ticks,
        checksum,
        0,
    ), line
    logging.info(line)

logging.info("VERIFIED %s frames; zero evaluation allocations", len(expected) * args.repeat)
connection.close()
