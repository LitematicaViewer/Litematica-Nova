import multiprocessing

from litematicaba.app import main


if __name__ == "__main__":
    # Windows 下 PyInstaller onefile 子进程会再次启动本 exe；须先于 GUI 入口调用。
    multiprocessing.freeze_support()
    raise SystemExit(main())
