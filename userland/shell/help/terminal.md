# What the terminal shows, and why

A terminal obeys control sequences in what it is sent. Text written by someone else (a file's
contents, a file's name, an agent's reply) could use them to set your clipboard, forge a link,
retitle the window, or make the terminal type as if you had. So the shell never sends one from
text: every control character is drawn as characters you can see.

  ^[  ESC, which starts most sequences     ^G  BEL
  ^M  a carriage return                    ^?  DEL
  <U+009B>  a C1 control, which some terminals obey as ESC [
  <U+202E>  a bidirectional override or isolate, which would reorder what is shown
  <FF>      a byte that is not UTF-8

A tab is drawn as spaces.

To see what a file really holds, look at its bytes:
  hexdump("strange.txt")
Each line is sixteen bytes in hex, with the printable ones beside them.

What a command returns, and every error, goes through the shell and is drawn safely. What a line
writes for itself, as IO.puts does, goes through the shell too and is drawn safely, and so does
what the VM logs, the crash reports of processes a line started among it.
