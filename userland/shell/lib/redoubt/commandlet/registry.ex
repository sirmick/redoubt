defmodule Redoubt.Commandlet.Registry do
  @moduledoc """
  Every commandlet of the shell, found by looking: a module of the shell's application that uses
  `Redoubt.Commandlet` joins it by being in the application, with nothing to list anywhere. Its
  commands are imported at the prompt (`imports/0`) and listed by `help`.

  The looking is done when the shell is built: the build's `commandlets` compiler (mix.exs) writes
  `Redoubt.Commandlet.Index`, which names the modules that declare commands and has a function for
  each command that calls its module's. The prompt imports the index, so nothing is loaded to find
  the commands, and a command's module is loaded when it is first called.
  """

  alias Redoubt.Commandlet

  # Written by the build after the modules it names: nothing to check at compile time.
  @compile {:no_warn_undefined, Redoubt.Commandlet.Index}
  @index Redoubt.Commandlet.Index

  @doc "Every commandlet, by name. It loads every module that declares one."
  @spec all() :: [Commandlet.t()]
  def all, do: modules() |> Enum.flat_map(& &1.__commandlets__()) |> Enum.sort_by(&Atom.to_string(&1.name))

  @doc "Every command's name, in order, from the index: no command's module is loaded."
  @spec names() :: [String.t()]
  def names, do: @index.__commands__() |> Map.keys() |> Enum.sort()

  @doc "The modules that declare commandlets, in order."
  @spec modules() :: [module()]
  def modules, do: @index.__modules__()

  @doc """
  The commandlet named `name`, an atom or a string. A string is compared as a string, so a name
  typed at the prompt never becomes an atom that did not exist. Only its own module is loaded.
  """
  @spec fetch(atom() | String.t()) :: {:ok, Commandlet.t()} | :error
  def fetch(name) when is_atom(name) or is_binary(name) do
    wanted = to_string(name)

    with {:ok, module} <- Map.fetch(@index.__commands__(), wanted),
         %Commandlet{} = commandlet <-
           Enum.find(module.__commandlets__(), &(Atom.to_string(&1.name) == wanted)) do
      {:ok, commandlet}
    else
      _ -> :error
    end
  end

  @doc """
  An `import` of every command, each arity of it, from the index, and nothing else of it, as
  quoted code for the prompt.
  """
  @spec imports() :: Macro.t()
  def imports, do: quote(do: import(unquote(@index), only: unquote(@index.__imports__())))
end
