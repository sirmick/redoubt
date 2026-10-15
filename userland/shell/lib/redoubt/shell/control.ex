defmodule Redoubt.Shell.Control do
  @moduledoc "The commands that control the shell itself, imported at the prompt."

  use Redoubt.Commandlet, area: "Shell"

  @summary "End the shell"
  @help """
  Ends the shell, as Ctrl+D does. Typed alone on a line, exit needs no parentheses.
  """
  @examples [{"exit()", "end the shell"}]
  defcommand exit() do
    # The evaluator catches this and tells the shell to end: the one way a line ends the loop.
    throw(:redoubt_shell_exit)
  end

  @summary "Enter or leave the machine mode"
  @help """
  Enters the machine mode, for a program driving the session over its terminal: from then on the
  lines typed are read as they are, with no prompt, echo, editing or history, until a line that is
  exactly token, which ends a block; the block is evaluated whole, and its end is marked with one
  line, `token n ok`, `token n error` or `token n exit`, n being the counter the prompt shows.
  The token is 8 to 64 characters of A-Z, a-z, 0-9, - and _; the code evaluated never sees it.
  Screens, the editor among them, are refused in the mode, and a long value prints whole, never
  in the pager. machine(nil) leaves the mode, and the next prompt follows its mark. Like exit(),
  the call ends its line: nothing else on the line is kept.
  """
  @args token: "the token that ends a block, or nil to leave the mode"
  @examples [{~S'machine("agent-7f3a9c")', "enter the mode"}, {"machine(nil)", "leave it"}]
  defcommand machine(token :: term) do
    cond do
      token == nil ->
        :ok

      not (is_binary(token) and token =~ ~r/\A[A-Za-z0-9_-]{8,64}\z/) ->
        raise Redoubt.Commandlet.UsageError,
          message: "machine: the token is 8 to 64 characters of A-Z, a-z, 0-9, - and _\nusage: machine(token)"

      Redoubt.Shell.Driver.of_group() == nil ->
        raise ArgumentError, "the machine mode needs the shell's terminal: this line's output is not it"

      true ->
        :ok
    end

    # The evaluator catches this and tells the shell, which holds the token in its loop's state.
    throw({:redoubt_shell_machine, token})
  end

  @summary "Clear the screen"
  @help """
  Clears the terminal and puts the cursor at its top left, where the next prompt is drawn. Output
  that is not the shell's terminal, a captured one or a file, is left as it is.
  """
  @examples [{"clear()", "a clean screen"}]
  defcommand clear() do
    case Redoubt.Shell.Driver.of_group() do
      nil ->
        :ok

      driver ->
        ref = make_ref()
        send(driver, {:redoubt_clear, self(), ref})

        receive do
          {:redoubt_cleared, ^ref} -> :ok
        end
    end
  end
end
