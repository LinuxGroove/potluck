#!/usr/bin/env python3
"""Watches every Potluck game, server and bot run on this machine and writes
down its memory, without touching it: RAM (resident, proportional and peak),
the graphics memory the driver holds for it, and its CPU, every few seconds.

    python3 tools/memory_watch.py [out dir] [seconds between samples]

Defaults: ~/potluck-memory, 2 s. Writes samples.csv (one line per process per
sample) and runs.txt (a line for each process when it ends: how long it ran,
its peaks and averages, and how it was started, so a gallery pass's tiers and
views can be told apart). Leave it running while other things play, shoot
and soak; it only reads /proc.

Graphics memory comes from the kernel's DRM fdinfo for amdgpu, i915 and xe
(on integrated graphics that's the same RAM, counted as gtt); NVIDIA's driver
doesn't keep one, so it asks `nvidia-smi -q -d PIDS`, and only while a
watched process has the NVIDIA card open (so it never wakes a sleeping card).
"""

import os
import subprocess
import sys
import time

NAMES = ("potluck", "potluck-server", "potluck-bots", "potluck-share")
CLK = os.sysconf("SC_CLK_TCK")
PAGE = os.sysconf("SC_PAGE_SIZE")
MB = 1024 * 1024


def read(path):
    try:
        with open(path, "rb") as f:
            return f.read().decode("utf-8", "replace")
    except OSError:
        return None


def kb_field(text, key):
    for line in text.splitlines():
        if line.startswith(key):
            parts = line.split()
            if len(parts) >= 2:
                return int(parts[1]) * 1024
    return 0


def watched():
    """Pids of Potluck processes, with their name and command line."""
    out = {}
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        comm = (read(f"/proc/{pid}/comm") or "").strip()
        if comm not in NAMES:
            continue
        args = (read(f"/proc/{pid}/cmdline") or "").replace("\0", " ").strip()
        out[int(pid)] = (comm, args)
    return out


def driver_memory(pid):
    """Graphics memory by region (vram, gtt, system0 ...) from DRM fdinfo,
    and whether the process has an NVIDIA device open."""
    regions, seen, nvidia = {}, set(), False
    try:
        fds = os.listdir(f"/proc/{pid}/fd")
    except OSError:
        return regions, nvidia
    for fd in fds:
        try:
            target = os.readlink(f"/proc/{pid}/fd/{fd}")
        except OSError:
            continue
        if target.startswith("/dev/nvidia"):
            nvidia = True
            continue
        if not target.startswith("/dev/dri/"):
            continue
        info = read(f"/proc/{pid}/fdinfo/{fd}") or ""
        client = next((l.split(":", 1)[1].strip() for l in info.splitlines() if l.startswith("drm-client-id:")), fd)
        if client in seen:
            continue
        seen.add(client)
        resident = "drm-resident-" in info
        for line in info.splitlines():
            key, _, value = line.partition(":")
            prefix = "drm-resident-" if resident else "drm-memory-"
            if not key.startswith(prefix):
                continue
            parts = value.split()
            if not parts or not parts[0].isdigit():
                continue
            unit = {"KiB": 1024, "MiB": MB, "GiB": 1024 * MB}.get(parts[1] if len(parts) > 1 else "", 1)
            region = key[len(prefix):]
            regions[region] = regions.get(region, 0) + int(parts[0]) * unit
    return regions, nvidia


def nvidia_memory():
    """Used GPU memory per pid from nvidia-smi, in bytes."""
    try:
        text = subprocess.run(["nvidia-smi", "-q", "-d", "PIDS"], capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return {}
    out, pid = {}, None
    for line in text.splitlines():
        key, _, value = line.partition(":")
        key, value = key.strip(), value.strip()
        if key == "Process ID" and value.isdigit():
            pid = int(value)
        elif key == "Used GPU Memory" and pid is not None:
            parts = value.split()
            if parts and parts[0].isdigit():
                out[pid] = out.get(pid, 0) + int(parts[0]) * MB
            pid = None
    return out


def cpu_ticks(pid):
    stat = read(f"/proc/{pid}/stat")
    if not stat:
        return None
    fields = stat.rsplit(")", 1)[1].split()
    # utime and stime are fields 14 and 15, counted from the pid
    return int(fields[11]) + int(fields[12])


def main():
    out = os.path.expanduser(sys.argv[1] if len(sys.argv) > 1 else "~/potluck-memory")
    every = float(sys.argv[2]) if len(sys.argv) > 2 else 2.0
    os.makedirs(out, exist_ok=True)
    samples = open(os.path.join(out, "samples.csv"), "a", buffering=1)
    runs = open(os.path.join(out, "runs.txt"), "a", buffering=1)
    if samples.tell() == 0:
        samples.write("time,pid,name,rss_mb,pss_mb,peak_mb,gpu_mb,gpu_regions,cpu_pct,threads,args\n")
    host = os.uname().nodename
    runs.write(f"# {time.strftime('%Y-%m-%d %H:%M:%S %Z')} watching on {host} every {every} s\n")
    live = {}  # pid -> dict of what we've seen
    while True:
        now = time.time()
        procs = watched()
        gpu_by_pid = {}
        drivers = {pid: driver_memory(pid) for pid in procs}
        if any(nv for _, nv in drivers.values()):
            gpu_by_pid = nvidia_memory()
        for pid, (name, args) in procs.items():
            status = read(f"/proc/{pid}/status")
            if not status:
                continue
            rss, peak = kb_field(status, "VmRSS:"), kb_field(status, "VmHWM:")
            threads = kb_field(status, "Threads:") // 1024
            rollup = read(f"/proc/{pid}/smaps_rollup") or ""
            pss = kb_field(rollup, "Pss:")
            regions, _ = drivers.get(pid, ({}, False))
            gpu = gpu_by_pid.get(pid, 0) + sum(regions.values())
            ticks = cpu_ticks(pid)
            r = live.setdefault(pid, {"name": name, "args": args, "start": now, "ticks": ticks, "last": (now, ticks), "peak_rss": 0, "peak_gpu": 0, "sum_rss": 0, "sum_gpu": 0, "n": 0, "regions": {}})
            last_t, last_ticks = r["last"]
            cpu = 0.0
            if ticks is not None and last_ticks is not None and now > last_t:
                cpu = 100.0 * (ticks - last_ticks) / CLK / (now - last_t)
            r["last"] = (now, ticks)
            r["args"] = args or r["args"]
            r["peak_rss"] = max(r["peak_rss"], rss, peak)
            r["peak_gpu"] = max(r["peak_gpu"], gpu)
            r["sum_rss"] += rss
            r["sum_gpu"] += gpu
            r["n"] += 1
            for k, v in regions.items():
                r["regions"][k] = max(r["regions"].get(k, 0), v)
            reg = " ".join(f"{k}={v / MB:.0f}" for k, v in sorted(regions.items()))
            if pid in gpu_by_pid:
                reg = (reg + " " if reg else "") + f"nvidia={gpu_by_pid[pid] / MB:.0f}"
            quoted = '"' + args.replace('"', "'") + '"'
            samples.write(f"{time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(now))},{pid},{name},{rss / MB:.0f},{pss / MB:.0f},{peak / MB:.0f},{gpu / MB:.0f},{reg},{cpu:.0f},{threads},{quoted}\n")
        for pid in [p for p in live if p not in procs]:
            r = live.pop(pid)
            secs = r["last"][0] - r["start"]
            n = max(r["n"], 1)
            cpu = ""
            if r["ticks"] is not None and r["last"][1] is not None and secs > 0:
                cpu = f", CPU {100.0 * (r['last'][1] - r['ticks']) / CLK / secs:.0f}%"
            reg = ", ".join(f"{k} {v / MB:.0f} MB" for k, v in sorted(r["regions"].items()))
            runs.write(
                f"{time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(r['start']))} {r['name']} {pid} ran {secs:.0f} s: "
                f"RAM peak {r['peak_rss'] / MB:.0f} MB (average {r['sum_rss'] / n / MB:.0f}), "
                f"GPU peak {r['peak_gpu'] / MB:.0f} MB (average {r['sum_gpu'] / n / MB:.0f}){' [' + reg + ']' if reg else ''}{cpu} | {r['args']}\n"
            )
        time.sleep(max(0.2, every - (time.time() - now)))


if __name__ == "__main__":
    main()
