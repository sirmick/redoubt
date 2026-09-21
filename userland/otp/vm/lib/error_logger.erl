%% A small stand-in for the kernel's error_logger: the old reporting API, forwarded to logger.
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(error_logger).
-export([error_msg/1, error_msg/2, format/2, warning_msg/1, warning_msg/2, info_msg/1, info_msg/2,
         error_report/1, error_report/2, warning_report/1, warning_report/2,
         info_report/1, info_report/2, get_format_depth/0, limit_term/1,
         add_report_handler/1, add_report_handler/2, delete_report_handler/1, tty/1]).

error_msg(Format) -> error_msg(Format, []).
error_msg(Format, Args) -> logger:log(error, Format, Args).
format(Format, Args) -> error_msg(Format, Args).
warning_msg(Format) -> warning_msg(Format, []).
warning_msg(Format, Args) -> logger:log(warning, Format, Args).
info_msg(Format) -> info_msg(Format, []).
info_msg(Format, Args) -> logger:log(info, Format, Args).

error_report(Report) -> logger:log(error, #{report => Report}).
error_report(Type, Report) -> logger:log(error, #{type => Type, report => Report}).
warning_report(Report) -> logger:log(warning, #{report => Report}).
warning_report(Type, Report) -> logger:log(warning, #{type => Type, report => Report}).
info_report(Report) -> logger:log(info, #{report => Report}).
info_report(Type, Report) -> logger:log(info, #{type => Type, report => Report}).

%% No depth limit on formatted terms.
get_format_depth() -> unlimited.
limit_term(Term) -> Term.

add_report_handler(_) -> ok.
add_report_handler(_, _) -> ok.
delete_report_handler(_) -> ok.
tty(_) -> ok.
