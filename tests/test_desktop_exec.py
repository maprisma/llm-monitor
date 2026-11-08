"""Check the installer's two-layer Desktop Entry Exec quoting without installing."""
import pathlib
import subprocess
import unittest


class DesktopExecTest(unittest.TestCase):
    def test_path_round_trip(self):
        script = (pathlib.Path(__file__).resolve().parents[1] / 'install.sh').read_text()
        function = script[script.index('desktop_arg()'):script.index('app_exec=')]
        for path in ['/tmp/simple/app', '/tmp/a b/"quoted"/`tick`/$dollar/50%/back\\slash']:
            with self.subTest(path=path):
                value = subprocess.check_output(
                    ['bash', '-c', function + '\ndesktop_arg "$1"', 'test', path], text=True)
                # String-value layer doubles every backslash.
                arg = value.replace('\\\\', '\\')
                self.assertTrue(arg.startswith('"') and arg.endswith('"'))
                arg = arg[1:-1]
                result = ''
                i = 0
                while i < len(arg):
                    if arg[i] == '\\':
                        i += 1
                        self.assertIn(arg[i], '\\"`$')
                    result += arg[i]
                    i += 1
                self.assertEqual(result.replace('%%', '%'), path)
