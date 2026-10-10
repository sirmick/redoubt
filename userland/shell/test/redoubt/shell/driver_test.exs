defmodule Redoubt.Shell.DriverTest do
  # The shell's helpers include cd, which moves the VM's working directory: run alone.
  use ExUnit.Case, async: false

  alias Redoubt.Test.Terminal

  @moduletag :tmp_dir

  # The whole stack on a terminal of this size: the driver, group and edlin, the shell. Keys
  # are typed with `type/2` and the screen read with `screen/1`.
  @cols 120
  @rows 12

  # The prompt shows the working directory, and the rows asserted assume it fits a row with the
  # line typed: the shell runs in /, as beamlet's tests do, not in the checkout, whose path would
  # wrap a long line on the BEAM in a deep enough checkout.
  setup do
    here = File.cwd!()
    File.cd!("/")
    on_exit(fn -> File.cd!(here) end)
  end

  # `opts` replace these (the driver takes an option's first value): a `size` of its own is the
  # driver's size, and the test's model must be made at it. `heap_words` is not the driver's: it
  # limits the driver's process as a session's VM does, the binaries it holds counted with its
  # heap.
  defp start(opts \\ [], rows \\ @rows) do
    test = self()
    {heap, opts} = Keyword.pop(opts, :heap_words)

    driver =
      spawn_link(fn ->
        if heap,
          do:
            Process.flag(:max_heap_size, %{
              size: heap,
              kill: true,
              error_logger: false,
              include_shared_binaries: true
            })

        Redoubt.Shell.Driver.run(
          Keyword.merge(
            [
              input: :messages,
              output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
              size: fn -> {@cols, rows} end,
              shell: {Redoubt.Shell, :start_link, [[banner: false]]}
            ],
            opts
          )
        )
      end)

    {driver, Terminal.new(@cols, rows)}
  end

  defp type(driver, bytes), do: send(driver, {:beamlet_console, bytes})

  # The screen once the shell has stopped drawing: what it wrote so far, fed to the model. With
  # `done`, drawing is waited for until the screen is done (10 s at most), however long the VM
  # takes to start it: beamlet's edlin handles a long read before it draws any of it.
  defp screen(terminal, done \\ fn _terminal -> true end) do
    receive do
      {:drawn, bytes} -> terminal |> Terminal.feed(bytes) |> screen(done)
    after
      if(done.(terminal), do: 300, else: 10_000) -> terminal
    end
  end

  # The bytes drawn until `pattern` matches the end of what was drawn (60 s at most), counted,
  # without a model: their count, and that end.
  defp drawn_until(pattern, bytes \\ 0, tail \\ "") do
    receive do
      {:drawn, drawn} ->
        tail =
          binary_part(
            tail <> drawn,
            max(byte_size(tail <> drawn) - 4096, 0),
            min(byte_size(tail <> drawn), 4096)
          )

        if tail =~ pattern,
          do: {bytes + byte_size(drawn), tail},
          else: drawn_until(pattern, bytes + byte_size(drawn), tail)
    after
      60_000 -> {bytes, tail}
    end
  end

  # A row's text, by its index from the top.
  defp row(terminal, i), do: terminal |> Terminal.lines() |> Enum.at(i, "")

  defp ends(driver) do
    ref = Process.monitor(driver)
    assert_receive {:DOWN, ^ref, :process, ^driver, :normal}, 5_000
  end

  # What Tab completes from is the prompt as it is: a variable bound before, a command, a path;
  # a second Tab lists. (The name is short: the test's directory, named for it, is the prompt.)
  test "Tab completes", %{tmp_dir: dir} do
    File.write!(Path.join(dir, "alpha.txt"), "")
    File.write!(Path.join(dir, "alps.txt"), "")
    here = File.cwd!()
    on_exit(fn -> File.cd!(here) end)

    # Wide, so the prompt's directory never wraps a row.
    {driver, _terminal} = start(size: fn -> {200, @rows} end)
    terminal = Terminal.new(200, @rows)
    text = fn terminal, pattern -> Terminal.text(terminal) =~ pattern end

    type(driver, ~s|cd("#{dir}")\r|)
    type(driver, "abcdef = 41\r")
    terminal = screen(terminal, &text.(&1, ~r/^41$/m))

    type(driver, "abc\t")
    terminal = screen(terminal, &text.(&1, ~r/\(3\)> abcdef$/m))
    type(driver, " + 1\r")
    terminal = screen(terminal, &text.(&1, ~r/^42$/m))
    assert text.(terminal, ~r/\(3\)> abcdef \+ 1$/m)

    type(driver, "hexd\t")
    terminal = screen(terminal, &text.(&1, ~r/\(4\)> hexdump\($/m))
    assert text.(terminal, ~r/\(4\)> hexdump\($/m)
    type(driver, "\x03")

    type(driver, ~s|cat("al\t|)
    terminal = screen(terminal, &text.(&1, ~r/> cat\("alp$/m))
    type(driver, "\t")
    terminal = screen(terminal, &text.(&1, ~r/alps\.txt/))
    assert text.(terminal, ~r/alpha\.txt +alps\.txt/)
    type(driver, "\x03")
    type(driver, "exit\r")
    ends(driver)
  end

  test "a line typed and entered runs, its value is printed below it, and the next prompt follows" do
    {driver, terminal} = start()
    type(driver, "1 + 1\r")
    terminal = screen(terminal)

    assert row(terminal, 0) =~ ~r/ \(1\)> 1 \+ 1$/
    assert row(terminal, 1) == "2"
    assert row(terminal, 2) =~ ~r/ \(2\)>$/
    # The cursor is after the prompt's space, which the rows' text trims.
    {2, col} = Terminal.cursor(terminal)
    assert col == String.length(row(terminal, 2)) + 1
    type(driver, "exit\r")
    ends(driver)
  end

  test "the line is edited in place: moving left, inserting, backspace and delete" do
    {driver, terminal} = start()
    # Ctrl+B is left; DEL is backspace; ESC [ 3 ~ is delete.
    type(driver, "1 +3")
    type(driver, "\x02 ")
    type(driver, "\r")
    type(driver, "99\x7F8\r")
    type(driver, "ab\x02\x02\e[3~\r")
    terminal = screen(terminal)

    assert row(terminal, 0) =~ ~r/\(1\)> 1 \+ 3$/
    assert row(terminal, 1) == "4"
    assert row(terminal, 2) =~ ~r/\(2\)> 98$/
    assert row(terminal, 3) == "98"
    assert row(terminal, 4) =~ ~r/\(3\)> b$/
    assert row(terminal, 5) =~ ~r/undefined variable "b"/
    type(driver, "exit\r")
    ends(driver)
  end

  test "history is in the session: the up arrow brings earlier lines back, Ctrl+R searches them" do
    {driver, terminal} = start()
    type(driver, "apple = 1\r")
    type(driver, "banana = 2\r")
    # Up twice is the first line again; Enter runs it. Each key is its own read: group drops
    # what follows a history key in the same read.
    terminal = screen(terminal)
    type(driver, "\e[A")
    terminal = screen(terminal)
    type(driver, "\e[A")
    terminal = screen(terminal)
    type(driver, "\r")
    terminal = screen(terminal)
    assert row(terminal, 4) =~ ~r/\(3\)> apple = 1$/
    assert row(terminal, 5) == "1"

    # Ctrl+R, part of a line, Enter to take it, Enter to run it.
    type(driver, "\x12ban")
    terminal = screen(terminal)
    assert row(terminal, 6) =~ ~r/^search: ban/
    type(driver, "\r")
    terminal = screen(terminal)
    assert row(terminal, 6) =~ ~r/\(4\)> banana = 2$/
    type(driver, "\r")
    terminal = screen(terminal)
    assert row(terminal, 7) == "2"
    type(driver, "exit\r")
    ends(driver)
  end

  test "history keeps the newest lines only, as many as the driver is told" do
    {driver, terminal} = start(history_lines: 2)
    type(driver, "apple = 1\r")
    terminal = screen(terminal)
    type(driver, "banana = 2\r")
    terminal = screen(terminal)
    type(driver, "cherry = 3\r")
    terminal = screen(terminal)

    # Up three times: banana is as far back as history goes, and the third is a beep.
    terminal =
      Enum.reduce(1..3, terminal, fn _, terminal ->
        type(driver, "\e[A")
        screen(terminal)
      end)

    assert row(terminal, 6) =~ ~r/\(4\)> banana = 2$/
    assert terminal.bells == 1
    type(driver, "\x03")
    type(driver, "exit\r")
    ends(driver)
  end

  test "Ctrl+C ends the line being typed, not the session, and the bindings before it stay" do
    {driver, terminal} = start()
    type(driver, "kept = :yes\r")
    type(driver, "1 +")
    terminal = screen(terminal)
    type(driver, "\x03")
    type(driver, "kept\r")
    terminal = screen(terminal)

    assert row(terminal, 2) =~ ~r/\(2\)> 1 \+\^C$/
    assert row(terminal, 3) =~ ~r/\(2\)> kept$/
    assert row(terminal, 4) == ":yes"
    assert Process.alive?(driver)
    type(driver, "exit\r")
    ends(driver)
  end

  test "Ctrl+\\ at the prompt is the interrupt too, and never reaches the line" do
    {driver, terminal} = start()
    type(driver, "1 +")
    terminal = screen(terminal)
    type(driver, "\x1C")
    type(driver, "\"a")
    terminal = screen(terminal)
    # What follows it in the same read goes with the line.
    type(driver, "\x1Cb\"\r")
    terminal = screen(terminal)

    assert row(terminal, 0) =~ ~r/\(1\)> 1 \+\^C$/
    assert row(terminal, 1) =~ ~r/\(1\)> "a\^C$/
    assert row(terminal, 2) =~ ~r/\(1\)>$/
    refute Terminal.text(terminal) =~ "^\\"
    type(driver, "exit\r")
    ends(driver)
  end

  test "clear() clears the screen, and the next prompt is drawn at its top" do
    {driver, terminal} = start()
    type(driver, "IO.puts(\"before\")\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ "before"))
    type(driver, "clear()\r")
    terminal = screen(terminal, &(not (Terminal.text(&1) =~ "before")))

    refute Terminal.text(terminal) =~ "before"
    assert row(terminal, 0) =~ ~r/^:ok$/
    assert row(terminal, 1) =~ ~r/\(3\)>$/
    type(driver, "exit\r")
    ends(driver)
  end

  # A session's VM gives each process a sixteenth of its budget's bytes, in 8-byte words: for the
  # image's 11,136-page share of a session, the driver's limit on the machine.
  @session_heap_words div(11_136 * 4096, 16 * 8)

  test "a printed line of 70,000 bytes is drawn within a session's heap limit, and the session goes on" do
    {driver, terminal} = start(heap_words: @session_heap_words)
    type(driver, "IO.puts(String.duplicate(\"z\", 70_000)); :seventy\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^:seventy$/m))

    assert Terminal.text(terminal) =~ ~r/^z{#{@cols}}$/m
    assert Terminal.text(terminal) =~ ~r/^:seventy$/m
    type(driver, "exit\r")
    ends(driver)
  end

  test "a printed line of 1 MiB, of text and of control characters, is drawn within a session's heap limit" do
    {driver, _terminal} = start(heap_words: @session_heap_words)
    type(driver, "IO.puts(String.duplicate(\"z\", 1_048_576)); :mib\r")
    {bytes, tail} = drawn_until(~r/\r\n:mib\r\n/)
    assert bytes > 1_048_576
    assert tail =~ ~r/\r\n:mib\r\n/

    # Each ESC is drawn as two characters, ^[.
    type(driver, "IO.puts(String.duplicate(<<27>>, 1_048_576)); :escapes\r")
    {bytes, tail} = drawn_until(~r/\r\n:escapes\r\n/)
    assert bytes > 2 * 1_048_576
    assert tail =~ ~r/\r\n:escapes\r\n/
    refute tail =~ "\e"
    assert Process.alive?(driver)
    type(driver, "exit\r")
    ends(driver)
  end

  test "Ctrl+C in the middle of an unfinished expression drops all of it" do
    {driver, terminal} = start()
    type(driver, "[1,\r")
    type(driver, "2,")
    terminal = screen(terminal)
    type(driver, "\x03")
    type(driver, "3\r")
    terminal = screen(terminal)

    assert row(terminal, 0) =~ ~r/\(1\)> \[1,$/
    assert row(terminal, 1) =~ ~r/^\.\.\.\(1\)> 2,\^C$/
    assert row(terminal, 2) =~ ~r/\(1\)> 3$/
    assert row(terminal, 3) == "3"
    type(driver, "exit\r")
    ends(driver)
  end

  # An interrupted line loses only itself (docs/plan/m2-usable-shell.md, the attack suite).
  test "Ctrl+C or Ctrl+\\ while a line is evaluated ends it, and the bindings before it stay" do
    {driver, terminal} = start()
    type(driver, "x = 1\r")
    type(driver, "Stream.cycle([1]) |> Stream.run()\r")
    Process.sleep(300)
    type(driver, "\x03")
    type(driver, "x + 1\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^2$/m))
    type(driver, "Stream.cycle([1]) |> Stream.run()\r")
    Process.sleep(300)
    type(driver, "\x1C")
    type(driver, "x + 2\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^3$/m))

    text = Terminal.text(terminal)
    assert text =~ ~r/Stream\.run\(\)\n\^C$/m
    assert text =~ ~r/^2$/m
    assert text =~ ~r/^3$/m
    assert Process.alive?(driver)
    type(driver, "exit\r")
    ends(driver)
  end

  test "the interrupt at the prompt after a line has ended ends no later line" do
    {driver, terminal} = start()
    type(driver, ":done\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ":done"))
    type(driver, "\x03")
    type(driver, "Process.sleep(300); :slept\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^:slept$/m))
    assert Terminal.text(terminal) =~ ~r/^:slept$/m
    type(driver, "exit\r")
    ends(driver)
  end

  test "Ctrl+D on an empty line ends the shell; with text on the line it deletes forward" do
    {driver, terminal} = start()
    type(driver, "ab\x02")
    terminal = screen(terminal)
    type(driver, "\x04\r")
    terminal = screen(terminal)
    assert row(terminal, 0) =~ ~r/\(1\)> a$/
    assert Process.alive?(driver)

    type(driver, "\x04")
    ends(driver)
  end

  # A line holding the feed, as a native program's input does (Redoubt.Pipeline): what it gets.
  @feed_line ~S"(fn -> :ok = Redoubt.Shell.Driver.open_feed(); f = fn f, acc -> receive do {:redoubt_feed, :data, t} -> f.(f, [t | acc]); {:redoubt_feed, :eof} -> Enum.reverse(acc) end end; f.(f, []) end).()"

  test "a line holding the feed gets the lines typed, edited and echoed, until Ctrl+D, and the shell goes on" do
    {driver, terminal} = start()
    type(driver, @feed_line <> "\r")
    Process.sleep(300)
    # A backspace takes a character back; no other control is fed.
    type(driver, "hello\r")
    type(driver, "wor\x7Fld\e\r")
    type(driver, "\x04")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~s|["hello\\n", "wold\\n"]|))
    text = Terminal.text(terminal)
    assert text =~ ~r/^hello$/m
    assert text =~ ~r/^wold$/m
    assert text =~ ~s|["hello\\n", "wold\\n"]|
    refute text =~ "^C"
    # The Ctrl+D ended the feed, not the shell: the next line runs.
    type(driver, "1 + 1\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^2$/m))
    assert Terminal.text(terminal) =~ ~r/^2$/m
    type(driver, "exit\r")
    ends(driver)
  end

  # No program swallows the interrupt: a line holding the feed, as a native program reading what
  # is typed does, is ended by it, and the keys typed with it are not fed.
  test "the interrupt under a feed ends the line that holds it, and nothing is fed" do
    {driver, terminal} = start()
    type(driver, "x = 1\r")
    type(driver, @feed_line <> "\r")
    Process.sleep(300)
    type(driver, "abc\x03def\r")
    type(driver, "x + 1\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^2$/m))

    text = Terminal.text(terminal)
    assert text =~ ~r/^abc\^C$/m
    refute text =~ "def"
    assert text =~ ~r/^2$/m
    type(driver, "exit\r")
    ends(driver)
  end

  test "a feed's end leaves what is typed after it to the shell" do
    {driver, terminal} = start()
    # The line takes one fed line and ends, holding the feed no more.
    type(
      driver,
      ~S|(fn -> :ok = Redoubt.Shell.Driver.open_feed(); receive do {:redoubt_feed, :data, t} -> t end end).()| <>
        "\r"
    )

    Process.sleep(300)
    type(driver, "only\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~s|"only\\n"|))
    type(driver, "40 + 2\r")
    terminal = screen(terminal, &(Terminal.text(&1) =~ ~r/^42$/m))
    assert Terminal.text(terminal) =~ ~r/^42$/m
    type(driver, "exit\r")
    ends(driver)
  end

  test "keys after a Ctrl+D in the same read are kept, in order, when it deletes forward" do
    {driver, terminal} = start()
    # At the prompt: a, b, left, Ctrl+D deletes the b, c, Enter, all in one read.
    terminal = screen(terminal)
    type(driver, "ab\x02\x04c\r")
    terminal = screen(terminal)
    assert row(terminal, 0) =~ ~r/\(1\)> ac$/
    assert row(terminal, 1) =~ ~r/undefined variable "ac"/
    type(driver, "exit\r")
    ends(driver)
  end

  test "the end of the input ends the shell, after the lines before it have run" do
    {driver, terminal} = start()
    type(driver, "x = 1\r")
    type(driver, "x + 1")
    type(driver, :eof)
    ends(driver)
    terminal = screen(terminal)
    assert row(terminal, 1) == "1"
    assert row(terminal, 3) == "2"
  end

  test "hostile text a line writes to the console itself never reaches the terminal as a control sequence" do
    {driver, terminal} = start()
    # Written by the line's own code, not through the printer: the one path the printer did
    # not guard. A clipboard write, a title, a status query, a bidirectional override, DEL.
    type(driver, ~S|IO.puts("\e]52;c;aGk=\a\e]0;pwned\a\e[6n\u202Eevil\x7F")| <> "\r")
    type(driver, ~S|IO.write("partial\e[2J")| <> "\r")
    terminal = screen(terminal)

    assert row(terminal, 1) == "^[]52;c;aGk=^G^[]0;pwned^G^[[6n<U+202E>evil^?"
    assert row(terminal, 2) =~ ~r/^:ok$/
    assert row(terminal, 4) == "partial^[[2J:ok"
    type(driver, "exit\r")
    ends(driver)
  end

  # Waits for the screen to hold `text`.
  defp shows(terminal, text), do: screen(terminal, &(Terminal.text(&1) =~ text))

  test "a crash report of a process a line spawned reaches the terminal as visible text" do
    # Tall enough to hold the three reports, stack traces and all.
    {driver, terminal} = start([], 200)
    # Through the emulator's report, through proc_lib's, and with a reason that is not UTF-8.
    type(driver, ~S|spawn(fn -> raise "\e]52;c;aGk=\a\e]0;pwned\a\u202Eevil\e[2J" end); :ok| <> "\r")
    terminal = shows(terminal, "evil")
    type(driver, ~S|Task.start(fn -> raise "task\e[6n\x9B" end); :ok| <> "\r")
    terminal = shows(terminal, "task")
    type(driver, ~S|spawn(fn -> :erlang.error({:boom, <<0xFF, 0xFE, 27, "[2J">>}) end); :ok| <> "\r")
    terminal = shows(terminal, "boom")
    # An event whose text is the controls themselves, unescaped by any formatter.
    type(driver, ~S|spawn(fn -> :logger.error("raw \e]52;c;aGk=\a\u202E\x9B<\xFF>") end); :ok| <> "\r")
    terminal = shows(terminal, "raw ")
    # The model raises on any sequence but the encoder's own; each report is drawn, in the words
    # of whichever formatter the VM's logger has (the BEAM's escapes the reasons itself).
    text = Terminal.text(terminal)
    assert text =~ ~r/pwned.*evil/
    assert text =~ ~r/Task #PID<[\d.]+> .*terminating/
    assert text =~ "boom"
    assert text =~ "raw ^[]52;c;aGk=^G<U+202E><9B><<FF>>"
    type(driver, "1 + 1\r")
    terminal = shows(terminal, ~r/^2$/m)
    assert Terminal.text(terminal) =~ ~r/^2$/m
    type(driver, "exit\r")
    ends(driver)
  end

  test "an event logged by group itself waits on nothing and is drawn as the fixed line" do
    {driver, terminal} = start()
    # Inside group's own process, while the line that asked waits on group.
    log = ~S|:logger.error("from group \e[2J")|
    type(driver, ~s|:sys.replace_state(Process.group_leader(), fn s -> #{log}; s end); :done| <> "\r")
    terminal = shows(terminal, "not shown")
    assert Terminal.text(terminal) =~ "[a log event from the shell's terminal, not shown]"
    refute Terminal.text(terminal) =~ "from group ^["
    type(driver, "1 + 1\r")
    terminal = shows(terminal, ~r/^2$/m)
    assert Terminal.text(terminal) =~ ":done"
    type(driver, "exit\r")
    ends(driver)
  end

  test "a process that logs as fast as it can holds a bounded backlog, and what was dropped is counted" do
    {driver, terminal} = start()
    # group's mailbox, sampled while the flood runs, kept in `deepest` and shown once the flood is
    # drawn; and the count of what was dropped.
    type(
      driver,
      ~S|spawn(fn -> for _ <- 1..2000, do: :logger.error("flood") end); deepest = Enum.max(for _ <- 1..20, do: (Process.sleep(5); elem(Process.info(Process.group_leader(), :message_queue_len), 1))); :sampled| <>
        "\r"
    )

    terminal = shows(terminal, "log events dropped")
    terminal = screen(terminal)
    type(driver, "{:deepest, deepest}\r")
    terminal = shows(terminal, ~r/^\{:deepest, \d+\}$/m)
    [_, deepest] = Regex.run(~r/^\{:deepest, (\d+)\}$/m, Terminal.text(terminal))
    assert String.to_integer(deepest) <= 4
    type(driver, "exit\r")
    ends(driver)
  end

  test "the logger's default handler is put back when the driver ends" do
    before = :logger.get_handler_ids()
    {driver, _terminal} = start()
    Process.sleep(100)
    assert :redoubt_shell in :logger.get_handler_ids()
    refute :default in :logger.get_handler_ids()
    type(driver, "exit\r")
    ends(driver)
    assert Enum.sort(:logger.get_handler_ids()) == Enum.sort(before)
  end

  test "hostile text typed or pasted is edited as text: a key sequence edlin does not know is dropped, the rest shown" do
    {driver, terminal} = start()
    type(driver, "\e]52;c;x\a\"ok\"\r")
    terminal = screen(terminal)
    assert row(terminal, 0) =~ ~r/\(1\)> .*"ok"$/
    refute Terminal.text(terminal) =~ "\e"
    type(driver, "exit\r")
    ends(driver)
  end

  test "a wide character typed takes two columns and the cursor follows it" do
    {driver, terminal} = start()
    type(driver, "\"世\"")
    terminal = screen(terminal)
    {0, col} = Terminal.cursor(terminal)
    assert col == String.length(row(terminal, 0)) + 1
    type(driver, "\r")
    terminal = screen(terminal)
    assert row(terminal, 1) == "\"世\""
    type(driver, "exit\r")
    ends(driver)
  end

  test "a UTF-8 sequence cut by the read's end is read whole, and a byte that is not UTF-8 as Latin-1" do
    {driver, terminal} = start()
    type(driver, "\"" <> <<0xE4, 0xB8>>)
    type(driver, <<0x96>> <> "\"\r")
    type(driver, <<"\"a", 0xFF, "b\"\r">>)
    terminal = screen(terminal)
    assert row(terminal, 1) == "\"世\""
    assert row(terminal, 3) == "\"aÿb\""
    type(driver, "exit\r")
    ends(driver)
  end

  test "a long line wraps at the terminal's width and the result starts on its own row" do
    {driver, terminal} = start()
    digits = String.duplicate("1", 130)
    type(driver, "String.length(\"#{digits}\")\r")
    terminal = screen(terminal, &(row(&1, 2) == "130"))
    assert String.length(row(terminal, 0)) == @cols
    assert row(terminal, 1) =~ ~r/1+"\)$/
    assert row(terminal, 2) == "130"
    type(driver, "exit\r")
    ends(driver)
  end

  test "a prompt wider than the terminal wraps, and what is typed after it is shown", %{tmp_dir: tmp_dir} do
    # A working directory long enough that the prompt, `<dir> (1)> `, is 300 columns.
    {:ok, home} = File.cwd()
    on_exit(fn -> File.cd!(home) end)
    base = Path.join(tmp_dir, "d")
    dir = base <> String.duplicate("x", 300 - String.length(base <> " (1)> "))
    File.mkdir_p!(dir)
    File.cd!(dir)

    {driver, terminal} = start()
    type(driver, "1 + 1")
    terminal = screen(terminal, &(Terminal.text(&1) =~ "1 + 1"))
    # 300 columns: two full rows and 60 columns of a third, where the typing follows.
    assert row(terminal, 0) == String.slice(dir, 0, @cols)
    assert row(terminal, 2) == String.slice(dir, 2 * @cols, @cols) <> " (1)> 1 + 1"
    assert Terminal.cursor(terminal) == {2, 60 + String.length("1 + 1")}

    # Ctrl+A goes back to the line's start, just after the prompt, and an insertion there redraws
    # the whole line.
    type(driver, "\x012")
    terminal = screen(terminal)
    assert row(terminal, 2) =~ ~r/ \(1\)> 21 \+ 1$/
    assert Terminal.cursor(terminal) == {2, 61}

    type(driver, "\r")
    terminal = screen(terminal)
    assert row(terminal, 3) == "22"
    type(driver, "exit\r")
    ends(driver)
  end

  test "a resize with no screen in front lays the line out again and is the console's size from then" do
    {driver, terminal} = start()
    type(driver, "1 + 1")
    terminal = screen(terminal, &(Terminal.text(&1) =~ "1 + 1"))
    send(driver, {:beamlet_console_resize, {100, 30}})
    # The line is drawn again, where it was.
    assert_receive {:drawn, redraw}, 5_000
    assert redraw =~ "1 + 1"
    terminal = terminal |> Terminal.feed(redraw) |> screen()
    assert row(terminal, 0) =~ ~r/\(1\)> 1 \+ 1$/
    # The size a screen would be given is the new one.
    send(driver, {:redoubt_screen, :size, self()})
    assert_receive {:redoubt_screen, :size, {100, 30}}, 5_000
    refute_received {:resize, _, _}
    type(driver, "\r")
    terminal = screen(terminal)
    assert row(terminal, 1) == "2"
    type(driver, "exit\r")
    ends(driver)
  end

  test "a resize with a screen in front is the screen's, as {:resize, cols, rows}" do
    {driver, terminal} = start()
    _terminal = screen(terminal)
    # This test's process is the screen.
    send(driver, {:redoubt_screen, :open, self(), :interrupt})
    assert_receive {:redoubt_screen, :opened, @cols, @rows}, 5_000
    send(driver, {:beamlet_console_resize, {100, 30}})
    assert_receive {:resize, 100, 30}, 5_000
    send(driver, {:redoubt_screen, :close, self()})
    type(driver, "exit\r")
    ends(driver)
  end
end
