defmodule Redoubt.Commandlet.Param do
  @moduledoc ~S"""
  A commandlet's parameter, and the types a parameter may have.

  | Type | Accepts |
  | --- | --- |
  | `path` | a file's name: a non-empty string with no NUL |
  | `string` | a string |
  | `integer` | an integer; `integer(min: 0)` and `max:` bound it |
  | `boolean` | `true` or `false` |
  | `pattern` | a string, found anywhere in a line, or a `Regex` |
  | `one_of([:a, :b])` | one of the atoms, or a string naming one; the parameter holds the atom |
  | `lines` | lines to go through: any enumerable but a string. Only the first parameter, where `\|>` puts them |
  | `command` | a commandlet's name, as an atom or a string; the parameter holds the commandlet |
  | `name` | a name, as an atom or a string; the parameter holds the string, so a command can look it up without making an atom |
  | `ref` | a module, or a function reference such as `&File.cp/2`; the parameter holds `{:module, m}` or `{:function, m, f, arity}` |
  | `handle` | a handle the session holds, as `ns_lookup/1` gives one: a resource term, which is a reference to Erlang code (a reference that is not a handle is refused by the native it reaches) |
  | `many(type)` | one `type`, or a list of them; the parameter holds a list. Only the last parameter, or the one before `flags` |
  | `flags(name: type, ...)` | options, as a keyword list: `sort(lines, reverse: true)`. The parameter holds a map with every flag, a flag not given being `false` if it is a boolean and `nil` if not. Only the last parameter; its default is `[]` |

  A value is checked, and coerced where the table says so, before the command's body runs. A name
  given as a string (to `one_of` or `command`) is compared as a string, so a name typed at the
  prompt never becomes an atom. A new type is its name in `@types` and a clause each in
  `accept/2`, `name/1` and `describe/1`.
  """

  alias Redoubt.Commandlet.Registry

  defstruct [:name, :type, :doc, default: :none]

  @type type :: {atom(), term()}
  @type t :: %__MODULE__{
          name: atom(),
          type: type(),
          doc: String.t() | nil,
          default: :none | {:default, String.t()}
        }

  @types [:path, :string, :integer, :boolean, :pattern, :lines, :command, :name, :ref, :handle]
  @options %{integer: [:min, :max]}

  @doc false
  # The type written in a declaration, as quoted: `path`, `integer(min: 0)`, `many(path)`,
  # `one_of([:a, :b])`, `flags(reverse: boolean)`.
  def type({:many, _meta, [inner]}) do
    case type(inner) do
      {:ok, {wrapper, _type}} when wrapper in [:many, :flags] ->
        {:error, "many(#{wrapper}(...)) is not a type"}

      {:ok, type} ->
        {:ok, {:many, type}}

      error ->
        error
    end
  end

  def type({:one_of, _meta, [values]}) when is_list(values) do
    if values != [] and Enum.all?(values, &is_atom/1),
      do: {:ok, {:one_of, values}},
      else: {:error, "one_of takes a list of atoms"}
  end

  def type({:flags, _meta, [flags]}) when is_list(flags) do
    if Keyword.keyword?(flags) and flags != [],
      do: flag_params(flags, []),
      else: {:error, "flags takes a keyword list of flag: type"}
  end

  def type({name, _meta, context}) when is_atom(name) and is_atom(context), do: base(name, [])
  def type({name, _meta, []}) when is_atom(name), do: base(name, [])
  def type({name, _meta, [opts]}) when is_atom(name) and is_list(opts), do: base(name, opts)
  def type(other), do: {:error, "#{Macro.to_string(other)} is not a type; " <> known()}

  defp flag_params([], params), do: {:ok, {:flags, Enum.reverse(params)}}

  defp flag_params([{name, type} | flags], params) do
    case flag_type(type) do
      {:ok, type} -> flag_params(flags, [%__MODULE__{name: name, type: type} | params])
      {:error, message} -> {:error, "flag #{name}: #{message}"}
    end
  end

  # A flag is any type but those that make no sense as an option.
  defp flag_type(type) do
    case type(type) do
      {:ok, {kind, _}} when kind in [:many, :flags, :lines] -> {:error, "a flag cannot be #{kind}"}
      result -> result
    end
  end

  defp base(name, opts) do
    unknown = Keyword.keys(opts) -- Map.get(@options, name, [])

    cond do
      name not in @types -> {:error, "#{name} is not a type; " <> known()}
      not Keyword.keyword?(opts) -> {:error, "#{name}'s options are a keyword list"}
      unknown != [] -> {:error, "#{name} has no option #{hd(unknown)}"}
      not Enum.all?(Keyword.values(opts), &is_integer/1) -> {:error, "#{name}'s options are integers"}
      true -> {:ok, {name, opts}}
    end
  end

  defp known, do: "the types are #{Enum.join(@types, ", ")}, one_of([...]), many(type) and flags(...)"

  @doc """
  Checks a value given for `param`: `{:ok, value}`, coerced where the type says so, or
  `{:error, hint}`, where the hint says more than the type does, or is `nil`, or is
  `{:message, text}` when the text says all there is to say.
  """
  @spec check(t(), term()) :: {:ok, term()} | {:error, String.t() | nil | {:message, String.t()}}
  def check(%__MODULE__{default: {:default, "nil"}}, nil), do: {:ok, nil}
  def check(%__MODULE__{type: type}, value), do: accept(type, value)

  defp accept({:many, type}, values) when is_list(values), do: all(values, &accept(type, &1))

  defp accept({:many, type}, value) do
    with {:ok, value} <- accept(type, value), do: {:ok, [value]}
  end

  defp accept({:flags, flags}, given) when is_list(given) do
    if Keyword.keyword?(given), do: flags(flags, given), else: {:error, nil}
  end

  defp accept({:path, _opts}, value) when is_binary(value) and value != "" do
    if String.contains?(value, <<0>>), do: {:error, "a path holds no NUL"}, else: {:ok, value}
  end

  defp accept({:string, _opts}, value) when is_binary(value), do: {:ok, value}

  defp accept({:handle, _opts}, value) when is_reference(value), do: {:ok, value}

  defp accept({:integer, opts}, value) when is_integer(value) do
    in_bounds = value >= Keyword.get(opts, :min, value) and value <= Keyword.get(opts, :max, value)
    if in_bounds, do: {:ok, value}, else: {:error, nil}
  end

  defp accept({:boolean, _opts}, value) when is_boolean(value), do: {:ok, value}

  defp accept({:pattern, _opts}, %Regex{} = regex), do: {:ok, regex}
  defp accept({:pattern, _opts}, value) when is_binary(value), do: {:ok, value}

  defp accept({:one_of, values}, value) when is_atom(value) or is_binary(value) do
    wanted = to_string(value)

    case Enum.find(values, &(Atom.to_string(&1) == wanted)) do
      nil -> {:error, nil}
      atom -> {:ok, atom}
    end
  end

  defp accept({:lines, _opts}, value) when is_binary(value),
    do: {:error, "a string is not lines: cat(path) reads a file's lines"}

  defp accept({:lines, _opts}, value),
    do: if(Enumerable.impl_for(value), do: {:ok, value}, else: {:error, nil})

  defp accept({:command, _opts}, value) when (is_atom(value) and value != nil) or is_binary(value) do
    case Registry.fetch(value) do
      {:ok, command} -> {:ok, command}
      :error -> {:error, "there is no command named #{value}; help() lists them"}
    end
  end

  defp accept({:name, _opts}, value) when is_atom(value) and value not in [nil, true, false],
    do: {:ok, Atom.to_string(value)}

  defp accept({:name, _opts}, value) when is_binary(value) and value != "", do: {:ok, value}

  defp accept({:ref, _opts}, value) when is_function(value) do
    case Function.info(value, :type) do
      {:type, :external} ->
        info = Function.info(value)
        {:ok, {:function, info[:module], info[:name], info[:arity]}}

      _local ->
        {:error, "an anonymous function has no documentation; name one, as &File.cp/2"}
    end
  end

  defp accept({:ref, _opts}, value) when is_atom(value) and value != nil do
    if Code.ensure_loaded?(value),
      do: {:ok, {:module, value}},
      else: {:error, "there is no module #{inspect(value)}"}
  end

  defp accept(_type, _value), do: {:error, nil}

  # Every flag, from what was given or its default, into a map; or the first thing wrong.
  defp flags(flags, given) do
    names = Enum.map(flags, & &1.name)
    keys = Keyword.keys(given)
    # Not `keys -- names`: that takes away one of each name, so a flag given twice would stay.
    unknown = Enum.reject(keys, &(&1 in names))
    twice = keys -- Enum.uniq(keys)

    cond do
      unknown != [] ->
        {:error, {:message, "there is no option #{hd(unknown)}; the options are #{Enum.join(names, ", ")}"}}

      twice != [] ->
        {:error, {:message, "the option #{hd(twice)} is given twice"}}

      true ->
        Enum.reduce_while(flags, {:ok, %{}}, fn flag, {:ok, map} ->
          case Keyword.fetch(given, flag.name) do
            :error ->
              {:cont, {:ok, Map.put(map, flag.name, flag_default(flag.type))}}

            {:ok, value} ->
              case accept(flag.type, value) do
                {:ok, value} ->
                  {:cont, {:ok, Map.put(map, flag.name, value)}}

                {:error, _hint} ->
                  got = inspect(value, limit: 5, printable_limit: 60)

                  {:halt,
                   {:error, {:message, "the option #{flag.name} must be #{describe(flag.type)}, got #{got}"}}}
              end
          end
        end)
    end
  end

  defp flag_default({:boolean, _opts}), do: false
  defp flag_default(_type), do: nil

  defp all(values, accept) do
    values
    |> Enum.reduce_while({:ok, []}, fn value, {:ok, acc} ->
      case accept.(value) do
        {:ok, value} -> {:cont, {:ok, [value | acc]}}
        error -> {:halt, error}
      end
    end)
    |> case do
      {:ok, acc} -> {:ok, Enum.reverse(acc)}
      error -> error
    end
  end

  @doc "The type's short name, for usage and help: `path`, `integer >= 0`, `path | [path]`."
  @spec name(type()) :: String.t()
  def name({:many, type}), do: "#{name(type)} | [#{name(type)}]"
  def name({:integer, opts}), do: "integer" <> bounds(opts[:min], opts[:max])
  def name({:one_of, values}), do: Enum.join(values, " | ")
  def name({:flags, _flags}), do: "options"
  def name({:ref, _opts}), do: "module | &fun/arity"
  def name({type, _opts}), do: Atom.to_string(type)

  defp bounds(nil, nil), do: ""
  defp bounds(min, nil), do: " >= #{min}"
  defp bounds(nil, max), do: " <= #{max}"
  defp bounds(min, max), do: " #{min}..#{max}"

  @doc "What the type accepts, for an error: `a path`, `an integer >= 0`."
  @spec describe(type()) :: String.t()
  def describe({:many, type}), do: "#{describe(type)}, or a list of them"
  def describe({:integer, _opts} = type), do: "an " <> name(type)
  def describe({:path, _opts}), do: "a path"
  def describe({:string, _opts}), do: "a string"
  def describe({:handle, _opts}), do: "a handle, as ns_lookup gives one"
  def describe({:boolean, _opts}), do: "true or false"
  def describe({:pattern, _opts}), do: "a string or a regex"
  def describe({:one_of, values}), do: "one of " <> Enum.map_join(values, ", ", &inspect/1)
  def describe({:lines, _opts}), do: "an enumerable of lines"
  def describe({:command, _opts}), do: "a command's name"
  def describe({:name, _opts}), do: "a name, as an atom or a string"
  def describe({:ref, _opts}), do: "a module, or a function reference such as &File.cp/2"

  def describe({:flags, flags}),
    do: "options: " <> Enum.map_join(flags, ", ", &"#{&1.name}: #{name(&1.type)}")

  @doc "How the parameter reads in a usage line: `count`, or `count \\\\ 10` with its default."
  @spec usage(t()) :: String.t()
  def usage(%__MODULE__{name: name, default: :none}), do: Atom.to_string(name)
  def usage(%__MODULE__{name: name, default: {:default, default}}), do: "#{name} \\\\ #{default}"
end
