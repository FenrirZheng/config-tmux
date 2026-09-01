#!/usr/bin/env python3
"""probe-sift-keys.py — drive a sift binary on a real pty and read its frames.

Two measurements, each with a control:

  keys    type a pattern, then send the literal bytes tmux sends for Home
          (ESC[1~), End (ESC[4~), PgUp (ESC[5~) and PgDn (ESC[6~); report the
          pattern the header shows and which hit row is selected after each.
          A binary that does not decode Home/End leaks the trailing `~` into
          the pattern (`aa0` -> `aa0~`) and the match count collapses.

  resize  send TIOCSWINSZ on the pty (the child has its own session and has
          claimed the pty as controlling terminal, so it gets a real SIGWINCH);
          report whether the process survived and the height of the frame it
          drew afterwards. `--no-resize` is the control: same timing, no ioctl.

usage: probe-sift-keys.py <keys|resize|no-resize> <path-to-sift-binary>
"""
import fcntl, os, pty, re, select, signal, struct, subprocess, sys, termios, time

SOCK_NAME = "sift_probe"
FIXTURE = os.path.expanduser(
    "~/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh")


def tmux(*a, sock=SOCK_NAME):
    env = dict(os.environ)
    env.pop("TMUX", None)
    return subprocess.run(["tmux", "-L", sock, *a], capture_output=True,
                          text=True, env=env)


def start_server():
    tmux("kill-server")
    tmux("-f", "/dev/null", "new-session", "-d", "-x", "100", "-y", "30")
    target = tmux("display-message", "-p", "#{pane_id}").stdout.strip()
    sockpath = tmux("display-message", "-p", "#{socket_path}").stdout.strip()
    tmux("send-keys", "-t", target, "bash '%s'" % FIXTURE, "Enter")
    time.sleep(1.5)
    return target, sockpath


def set_winsize(fd, rows, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def spawn(binary, pane, sockpath, rows=30, cols=100):
    master, slave = pty.openpty()
    set_winsize(slave, rows, cols)
    env = dict(os.environ)
    env["TMUX"] = "%s,0,0" % sockpath          # throwaway socket only
    env["TERM"] = "xterm-256color"

    def child():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)   # real controlling terminal

    p = subprocess.Popen([binary, pane], stdin=slave, stdout=slave,
                         stderr=slave, preexec_fn=child, env=env, close_fds=True)
    os.close(slave)
    return p, master


def drain(fd, quiet=0.4, limit=3.0):
    """Read until the child has been silent for `quiet` seconds."""
    buf = b""
    deadline = time.time() + limit
    last = time.time()
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
            last = time.time()
        elif time.time() - last > quiet:
            break
    return buf


CLS = b"\x1b[H\x1b[2J"


def last_frame(buf):
    return buf.split(CLS)[-1] if CLS in buf else buf


def header(frame):
    m = re.search(rb"\x1b\[1mregex> (.*?)\x1b\[0m", frame, re.S)
    return m.group(1) if m else b"<no header>"


def status(frame):
    m = re.search(rb"\x1b\[2m([^\x1b]*)\x1b\[0m\r\n", frame)
    return m.group(1).strip() if m else b"<no status>"


def selected(frame):
    m = re.search(rb"\x1b\[1m> \x1b\[2m\s*(\d+)", frame)
    return int(m.group(1)) if m else None


def height(frame):
    # header + (h-2) list rows each end in \r\n; the footer has none.
    return frame.count(b"\r\n") + 1


def report(tag, frame):
    print("  %-10s pattern=%-8s status=%-28s sel_line=%s height=%s"
          % (tag, header(frame).decode("utf8", "replace"),
             status(frame).decode("utf8", "replace"),
             selected(frame), height(frame)))


def main():
    mode, binary = sys.argv[1], sys.argv[2]
    target, sockpath = start_server()
    p, m = spawn(binary, target, sockpath)
    try:
        drain(m)                                   # startup frame
        os.write(m, b"aa0"); time.sleep(0.3)
        f = last_frame(drain(m))
        print("%s [%s]" % (mode, os.path.basename(binary)))
        report("typed aa0", f)
        base_sel, base_h = selected(f), height(f)

        if mode in ("keys", "keys-rxvt", "resize-then-key"):
            if mode == "resize-then-key":
                set_winsize(m, 40, 120); time.sleep(0.5)
                report("after resize", last_frame(drain(m)))
                print("  alive after resize=%s" % (p.poll() is None))
            seqs = (("Home ESC[1~", b"\x1b[1~"), ("End ESC[4~", b"\x1b[4~"),
                    ("PgUp ESC[5~", b"\x1b[5~"), ("PgDn ESC[6~", b"\x1b[6~"))
            if mode == "keys-rxvt":
                seqs = (("Home ESC[7~", b"\x1b[7~"), ("End ESC[8~", b"\x1b[8~"),
                        ("Home ESC[H", b"\x1b[H"), ("End ESC OF", b"\x1bOF"))
            if mode == "resize-then-key":
                seqs = (("Home ESC[1~", b"\x1b[1~"), ("End ESC[4~", b"\x1b[4~"))
            for tag, seq in seqs:
                os.write(m, seq); time.sleep(0.35)
                report(tag, last_frame(drain(m)))
            alive = p.poll() is None
            print("  alive=%s" % alive)
        else:
            if mode == "resize":
                set_winsize(m, 40, 120)
            time.sleep(0.6)
            out = drain(m)
            alive = p.poll() is None
            f2 = last_frame(out) if out else f
            print("  after %s: alive=%s exit=%s" % (mode, alive, p.poll()))
            if out:
                report("post frame", f2)
            else:
                print("  post frame: (nothing drawn)  baseline height=%s" % base_h)
            print("  RESULT alive=%s new_height=%s (was %s) drew=%s"
                  % (alive, height(f2) if out else None, base_h, bool(out)))
    finally:
        try:
            p.send_signal(signal.SIGKILL)
        except Exception:
            pass
        os.close(m)
        tmux("kill-server")


main()
