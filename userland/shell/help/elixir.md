# Elixir at the prompt

The prompt is Elixir, and only Elixir: what works in a script works here, and there is no other
syntax to learn.

Calling a command
  A call with no arguments keeps its parentheses: pwd(), ls(), help(). A bare pwd is a
  variable, and the shell says so. With arguments, the parentheses may go: cd "logs" and
  cp "a.txt", "b.txt" work as they read.

Text and names
  Text is in double quotes: "app.log". A name that starts with a colon is an atom, a fixed
  name: help(:grep). Single quotes are not strings.

Pipes
  |> hands the value on the left to the call on the right, as its first argument:
    cat("app.log") |> grep("error") |> count()
  Put the parentheses on the first call. cat "app.log" |> grep("x") reads as
  cat("app.log" |> grep("x")), which is not what it looks like.

Options
  Options come last, as name: value: sort(lines, reverse: true), grep(lines, "x", ignore_case: true).

Several lines
  An expression that is not finished (defmodule M do, an open bracket, an open string) goes on
  the next line; the prompt shows ...(n)> until it is.

Values last
  x = cat("app.log") |> grep("error") keeps x for the lines that follow. A line that fails, or is
  killed, loses only itself: x is still there.

Stopping
  Ctrl+C drops the line being typed. Ctrl+\ does too, and also ends any full-screen program,
  even one that takes Ctrl+C as a key.

Leaving
  exit, exit() or Ctrl+D.
