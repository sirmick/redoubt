defmodule Redoubt.UtilTest do
  use ExUnit.Case, async: true

  import Redoubt.Util
  import Redoubt.Test.Seed

  alias Redoubt.Commandlet.UsageError

  @moduletag :tmp_dir

  # Absolute paths only: these run beside other tests, and must not depend on the working directory.
  defp at(dir, name), do: Path.join(dir, name)

  defp write(dir, name, text) do
    path = Path.join(dir, name)
    File.write!(path, text)
    path
  end

  test "cat gives a file's lines, without their endings", %{tmp_dir: dir} do
    path = write(dir, "a.txt", "one\ntwo\r\nthree")
    assert Enum.to_list(cat(path)) == ["one", "two", "three"]
  end

  test "cat of several files gives their lines in order", %{tmp_dir: dir} do
    a = write(dir, "a.txt", "1\n2\n")
    b = write(dir, "b.txt", "3\n")
    assert Enum.to_list(cat([a, b])) == ["1", "2", "3"]
  end

  test "a missing file fails when cat is called, not when its lines are read", %{tmp_dir: dir} do
    assert_raise File.Error, ~r/no such file/, fn -> cat(Path.join(dir, "missing.txt")) end
    assert_raise File.Error, fn -> cat([write(dir, "here.txt", "x"), Path.join(dir, "gone.txt")]) end
    assert_raise File.Error, ~r/directory/, fn -> cat(dir) end
  end

  test "grep keeps the lines holding a string or matching a regex, and count counts them", %{tmp_dir: dir} do
    path = write(dir, "app.log", "ok start\nerror disk\nok run\nerror net\n")
    assert cat(path) |> grep("error") |> Enum.to_list() == ["error disk", "error net"]
    assert cat(path) |> grep(~r/^ok /) |> count() == 2
    assert cat(path) |> grep("nothing") |> count() == 0
  end

  test "head takes the first lines, and stops reading there", %{tmp_dir: dir} do
    path = write(dir, "many.txt", Enum.map_join(1..1000, &"line #{&1}\n"))
    assert cat(path) |> head(2) |> Enum.to_list() == ["line 1", "line 2"]
    assert cat(path) |> head() |> count() == 10
  end

  test "lines are not read until something reads them", %{tmp_dir: dir} do
    path = write(dir, "later.txt", "before\n")
    lines = cat(path)
    File.write!(path, "after\n")
    assert Enum.to_list(lines) == ["after"]
    assert inspect(lines) == "#Lines<...>"
  end

  test "grep and grep_v find a string whatever its case when asked, and a regex never", %{tmp_dir: dir} do
    log = at(seed(dir), "logs/app.log")
    assert cat(log) |> grep("error") |> count() == 0

    assert cat(log) |> grep("error", ignore_case: true) |> Enum.to_list() == [
             "ERROR 12 disk full",
             "ERROR 7 net down"
           ]

    assert cat(log) |> grep_v("INFO") |> Enum.to_list() == [
             "ERROR 12 disk full",
             "WARN  slow",
             "ERROR 7 net down"
           ]

    assert cat(log) |> grep_v("info", ignore_case: true) |> count() == 3

    assert_raise UsageError, ~r/grep: ignore_case is for a string/, fn ->
      cat(log) |> grep(~r/error/, ignore_case: true)
    end
  end

  test "tail keeps the last lines", %{tmp_dir: dir} do
    notes = at(seed(dir), "notes.txt")
    assert cat(notes) |> tail(2) |> Enum.to_list() == ["second", "third"]
    assert cat(notes) |> tail() |> count() == 3
    assert cat(notes) |> tail(0) |> Enum.to_list() == []
  end

  test "sort is by bytes, or by leading number, and can be reversed", %{tmp_dir: dir} do
    fruit = at(seed(dir), "docs/fruit.txt")
    assert cat(fruit) |> sort() |> Enum.to_list() == ["10", "9", "apple", "apple", "banana", "cherry"]

    assert cat(fruit) |> sort(reverse: true) |> Enum.to_list() == [
             "cherry",
             "banana",
             "apple",
             "apple",
             "9",
             "10"
           ]

    assert cat(fruit) |> sort(numeric: true) |> Enum.to_list() == [
             "apple",
             "apple",
             "banana",
             "cherry",
             "9",
             "10"
           ]
  end

  test "uniq drops a line the same as the one before, and uniq_c counts each run" do
    assert ["a", "a", "b", "a"] |> uniq() |> Enum.to_list() == ["a", "b", "a"]
    assert ["b", "a", "b", "a"] |> sort() |> uniq() |> Enum.to_list() == ["a", "b"]
    assert ["a", "a", "b", "a"] |> uniq_c() == [{2, "a"}, {1, "b"}, {1, "a"}]
  end

  test "sub replaces every match, a regex's groups by number", %{tmp_dir: dir} do
    log = at(seed(dir), "logs/app.log")
    assert ["a-a-a"] |> sub("-", "+") |> Enum.to_list() == ["a+a+a"]

    assert cat(log) |> grep("ERROR") |> sub(~r/ERROR (\d+)/, "E\\1") |> Enum.to_list() == [
             "E12 disk full",
             "E7 net down"
           ]
  end

  test "cut keeps fields by number, in the order asked, a missing one empty" do
    lines = ["a:b:c", "d:e"]
    assert lines |> cut(":", 2) |> Enum.to_list() == ["b", "e"]
    assert lines |> cut(":", [3, 1]) |> Enum.to_list() == ["c:a", ":d"]
    assert_raise UsageError, ~r/cut: fields must be an integer >= 1/, fn -> cut(lines, ":", 0) end
  end

  test "w replaces a file through a new one, so a file can be read and written back", %{tmp_dir: dir} do
    notes = at(seed(dir), "notes.txt")
    assert cat(notes) |> sub("second", "2nd") |> w(notes) == :ok
    assert File.read!(notes) == "first\n2nd\nthird\n"
    assert ["x", 1, :y] |> w(at(dir, "new.txt")) == :ok
    assert File.read!(at(dir, "new.txt")) == "x\n1\ny\n"
    refute Enum.any?(File.ls!(dir), &String.ends_with?(&1, ".tmp"))
  end

  test "a w that fails leaves the old file whole, and nothing beside it", %{tmp_dir: dir} do
    seed(dir)
    assert_raise File.RenameError, fn -> w(["x"], at(dir, "logs")) end
    assert File.dir?(at(dir, "logs/old"))
    refute Enum.any?(File.ls!(dir), &String.ends_with?(&1, ".tmp"))
  end

  test "append adds to a file, and makes one", %{tmp_dir: dir} do
    notes = at(seed(dir), "notes.txt")
    assert append(["fourth"], notes) == :ok
    assert File.read!(notes) == "first\nsecond\nthird\nfourth\n"
    assert append(["only"], at(dir, "fresh.txt")) == :ok
    assert File.read!(at(dir, "fresh.txt")) == "only\n"
  end

  test "hexdump shows every byte, and never a control character", %{tmp_dir: dir} do
    seed(dir)
    rows = Enum.to_list(hexdump(at(dir, "data.bin")))

    assert hd(rows) == "00000000  00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f  |................|"

    assert Enum.at(rows, 4) ==
             "00000040  40 41 42 43 44 45 46 47  48 49 4a 4b 4c 4d 4e 4f  |@ABCDEFGHIJKLMNO|"

    assert List.last(rows) == "00000100"
    assert length(rows) == 17
    assert Enum.all?(rows, &(&1 =~ ~r/\A[\x20-\x7e]*\z/))

    [first, second, size] = Enum.to_list(hexdump(at(dir, "notes.txt")))
    assert first =~ ~r/^00000000  66 69 72 73 74 0a 73 65  63 6f 6e 64 0a 74 68 69  \|first.second.thi\|$/
    assert second =~ ~r/^00000010  72 64 0a +\|rd.\|$/
    assert size == "00000013"

    assert Enum.to_list(hexdump(at(dir, "empty.txt"))) == ["00000000"]
    assert_raise File.Error, fn -> hexdump(at(dir, "missing")) end
  end

  # The hashes are sha256sum's and sha512sum's, not the VM's own: a VM that hashed wrongly would
  # agree with itself.
  test "checksum hashes a file with SHA-256, or SHA-512", %{tmp_dir: dir} do
    notes = at(seed(dir), "notes.txt")
    assert checksum(notes) == "f5c962601b413ccda2fc14d64d98479d9fc74c90c2dde15f25ee9922e57f5074"

    assert checksum(notes, :sha512) ==
             "d89171f486a25433814a2a6156bda24d3b48895548ebe2174f06b80e38892bda" <>
               "08a19c25aaa5e2dfd58d1771a1568e94554f8bb7f3e9741116c2aa8032f848cb"

    assert checksum(notes, "sha256") == checksum(notes)

    assert checksum(at(dir, "empty.txt")) ==
             "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

    assert_raise UsageError, ~r/algorithm must be one of :sha256, :sha512, got :md5/, fn ->
      checksum(notes, :md5)
    end
  end

  # follow prints into its process's group leader; here a StringIO stands in for the console.
  defp following(path) do
    {:ok, out} = StringIO.open("")
    test = self()

    pid =
      spawn(fn ->
        Process.group_leader(self(), out)
        send(test, {:followed, follow(path)})
      end)

    {pid, out}
  end

  defp printed(out), do: out |> StringIO.contents() |> elem(1)

  test "follow prints the lines added from now on, a line not yet ended once it is, until killed",
       %{tmp_dir: dir} do
    path = write(dir, "app.log", "before\n")
    {pid, out} = following(path)
    Process.sleep(100)
    File.write!(path, "one\ntw", [:append])
    Process.sleep(1_200)
    assert printed(out) == "one\n"
    File.write!(path, "o\nthree\n", [:append])
    Process.sleep(1_200)
    assert printed(out) == "one\ntwo\nthree\n"
    # The interrupt kills the line's process: nothing is left of follow.
    Process.exit(pid, :kill)
    refute_receive {:followed, _}
  end

  test "follow prints a line not yet ended as it stands once it passes 4 KiB", %{tmp_dir: dir} do
    path = write(dir, "app.log", "")
    {pid, out} = following(path)
    Process.sleep(100)
    long = String.duplicate("x", 4097)
    File.write!(path, long, [:append])
    Process.sleep(1_200)
    assert printed(out) == long <> "\n"
    File.write!(path, "y\n", [:append])
    Process.sleep(1_200)
    assert printed(out) == long <> "\ny\n"
    Process.exit(pid, :kill)
  end

  test "follow ends by itself when its file gets shorter or goes away", %{tmp_dir: dir} do
    path = write(dir, "a.log", "some text\n")
    {_pid, _out} = following(path)
    Process.sleep(100)
    File.write!(path, "")
    assert_receive {:followed, :truncated}, 2_000

    {_pid, _out} = following(path)
    Process.sleep(100)
    File.rm!(path)
    assert_receive {:followed, :removed}, 2_000
    assert_raise File.Error, fn -> follow(at(dir, "missing.log")) end
  end
end
