defmodule Redoubt.Shell.HelpersTest do
  # cd changes the VM's working directory, which every test shares: never run beside another.
  use ExUnit.Case, async: false

  import Redoubt.Shell.Helpers
  import Redoubt.Util
  import Redoubt.Test.Seed

  alias Redoubt.Commandlet.UsageError
  alias Redoubt.Shell.Stat

  @moduletag :tmp_dir

  setup do
    here = File.cwd!()
    on_exit(fn -> File.cd!(here) end)
  end

  test "cd moves where relative names resolve, and pwd says where", %{tmp_dir: dir} do
    File.mkdir_p!(Path.join(dir, "logs"))
    File.write!(Path.join(dir, "logs/app.log"), "hello\n")

    assert cd(dir) == pwd()
    assert cd("logs") == Path.join(dir, "logs")
    assert Enum.to_list(cat("app.log")) == ["hello"]
    assert_raise File.Error, fn -> cd("nowhere") end
    assert pwd() == Path.join(dir, "logs")
  end

  test "ls gives a directory's names in order", %{tmp_dir: dir} do
    for name <- ["b.txt", "a.txt", "c"], do: File.write!(Path.join(dir, name), "")
    assert Enum.to_list(ls(dir)) == ["a.txt", "b.txt", "c"]
  end

  test "cp copies, and into a directory keeps the name", %{tmp_dir: dir} do
    cd(dir)
    File.write!("a.txt", "data\n")
    File.mkdir!("backup")

    assert cp("a.txt", "b.txt") == :ok
    assert cp("a.txt", "backup") == :ok
    assert File.read!("b.txt") == "data\n"
    assert File.read!("backup/a.txt") == "data\n"
    assert File.read!("a.txt") == "data\n"
  end

  test "mv renames, and into a directory keeps the name", %{tmp_dir: dir} do
    cd(dir)
    File.write!("old.txt", "data\n")
    File.mkdir!("archive")

    assert mv("old.txt", "new.txt") == :ok
    refute File.exists?("old.txt")
    assert mv("new.txt", "archive") == :ok
    assert File.read!("archive/new.txt") == "data\n"
    assert_raise File.RenameError, fn -> mv("missing.txt", "x.txt") end
  end

  test "mv across volumes copies, then removes, a file or a whole directory", %{tmp_dir: dir} do
    other = other_volume() || flunk("no second volume: ./test-shell mounts one at /xdev")
    cd(seed(dir))

    assert mv("notes.txt", other) == :ok
    refute File.exists?("notes.txt")
    assert File.read!(Path.join(other, "notes.txt")) == "first\nsecond\nthird\n"

    assert mv("logs", other) == :ok
    refute File.exists?("logs")
    assert File.read!(Path.join(other, "logs/old/2024.log")) == "old entry\n"

    # A directory never lands on one already there, as a rename would refuse.
    File.mkdir_p!("logs/new")
    assert_raise File.RenameError, fn -> mv("logs", other) end
    assert File.dir?("logs/new")
  end

  test "ls_r lists everything below, a directory before what it holds", %{tmp_dir: dir} do
    cd(seed(dir))

    assert Enum.to_list(ls_r()) == [
             "data.bin",
             "docs",
             "docs/fruit.txt",
             "empty.txt",
             "evil\e[2J.txt",
             "logs",
             "logs/app.log",
             "logs/dos.log",
             "logs/old",
             "logs/old/2024.log",
             "notes.txt"
           ]

    assert Enum.to_list(ls_r("logs")) == ["logs/app.log", "logs/dos.log", "logs/old", "logs/old/2024.log"]
  end

  test "find matches names by string or regex, at every depth", %{tmp_dir: dir} do
    cd(seed(dir))
    assert Enum.to_list(find(".", ".log")) == ["logs/app.log", "logs/dos.log", "logs/old/2024.log"]
    assert Enum.to_list(find("logs", ~r/^\d+\.log$/)) == ["logs/old/2024.log"]
    assert Enum.to_list(find(".", "nothing")) == []
  end

  test "glob expands a wildcard, in order", %{tmp_dir: dir} do
    cd(seed(dir))
    assert Enum.to_list(glob("logs/*.log")) == ["logs/app.log", "logs/dos.log"]
    assert Enum.to_list(glob("**/*.log")) == ["logs/app.log", "logs/dos.log", "logs/old/2024.log"]
    assert Enum.to_list(glob("*.none")) == []
  end

  test "stat gives the type, size and time, and no mode or owner", %{tmp_dir: dir} do
    cd(seed(dir))
    assert %Stat{path: "notes.txt", type: :regular, size: 19, mtime: %DateTime{}} = stat("notes.txt")
    assert %Stat{type: :directory} = stat("logs")
    assert Map.keys(Map.from_struct(stat("notes.txt"))) |> Enum.sort() == [:mtime, :path, :size, :type]
    assert_raise File.Error, fn -> stat("missing") end
  end

  test "rm removes a file or an empty directory, and leaves a full one", %{tmp_dir: dir} do
    cd(seed(dir))
    File.mkdir!("hollow")

    assert rm("notes.txt") == :ok
    assert rm("hollow") == :ok
    refute File.exists?("notes.txt") or File.exists?("hollow")

    assert_raise File.Error, ~r/remove directory "logs"/, fn -> rm("logs") end
    assert File.exists?("logs/app.log")
    assert_raise File.Error, fn -> rm("missing") end
  end

  test "rm_rf removes a tree, and succeeds when there is nothing", %{tmp_dir: dir} do
    cd(seed(dir))
    assert rm_rf("logs") == :ok
    refute File.exists?("logs")
    assert rm_rf("missing") == :ok
  end

  # Never by calling rm_rf("/"): if the guard were wrong, the test would remove the machine's files.
  test "rm_rf refuses /, however it is written, from wherever it is", %{tmp_dir: dir} do
    assert root?("/") and root?("/..") and root?("//") and root?("/tmp/..")

    cd(dir)
    refute root?("/tmp") or root?("tmp") or root?(".")

    # From /, as a session on beamlet starts, a relative path can be / too.
    cd("/")
    assert root?(".") and root?("tmp/..")

    assert_raise UsageError, "rm_rf: path must be a path, got \"\"\nusage: rm_rf(path)", fn -> rm_rf("") end
  end

  test "mkdir makes one directory, mkdir_p every one missing", %{tmp_dir: dir} do
    cd(dir)
    assert mkdir("a") == :ok
    assert_raise File.Error, fn -> mkdir("a") end
    assert_raise File.Error, fn -> mkdir("b/c") end
    assert mkdir_p("b/c/d") == :ok
    assert mkdir_p("b/c/d") == :ok
    assert File.dir?("b/c/d")
  end

  test "touch makes an empty file, and leaves what a file holds", %{tmp_dir: dir} do
    cd(seed(dir))
    assert touch("new.txt") == :ok
    assert File.read!("new.txt") == ""
    assert touch("notes.txt") == :ok
    assert File.read!("notes.txt") == "first\nsecond\nthird\n"
  end
end
