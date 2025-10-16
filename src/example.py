import shutil
import subprocess
import winreg
import os
import sys
import pathlib
import re


# modify the Register
def write_key():
    # add cmd for zip files
    with winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, r"SystemFileAssociations\.zip", 0, winreg.KEY_WRITE) as key:
        cmd_subkey = winreg.CreateKey(key, r"shell\Open Signals\command")
        with winreg.OpenKey(cmd_subkey, "", 0, winreg.KEY_WRITE) as cmd_key:
            script_path = os.path.join(os.path.dirname(sys.executable), sys.argv[0])
            cmd_value = f'"{script_path}"' + ' "%1"'
            winreg.SetValue(cmd_key, '', winreg.REG_SZ, cmd_value)

    # add cmd for directory
    with winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, r"Directory\shell", 0, winreg.KEY_WRITE) as key:
        cmd_subkey = winreg.CreateKey(key, r"Open Signal\command")
        with winreg.OpenKey(cmd_subkey, "", 0, winreg.KEY_WRITE) as cmd_key:
            script_path = os.path.join(os.path.dirname(sys.executable), sys.argv[0])
            cmd_value = f'"{script_path}"' + ' "%1"'
            winreg.SetValue(cmd_key, '', winreg.REG_SZ, cmd_value)


# waves operation
class Waves:
    def __init__(self, path_list):
        self.waves_list = path_list
        self.pwd = os.path.dirname(sys.executable)
        self.signal_list = []
        self.tcl_file = os.path.join(os.getcwd(),"add_signal.tcl")
        self.readSignalList()
        self.tcl_template = '''
proc add_sig {} {
    set nfacs [ gtkwave::getNumFacs ]
    set all_facs [list]
    for {set i 0} {$i < $nfacs } {incr i} {
        set facname [gtkwave::getFacName $i]
        set facname2 [gtkwave::getFacName $i]
        set changes [ gtkwave::signalChangeList $facname2 -max 1 ]
        set no_x 1
        foreach {time value} $changes {
            set firststr [string range $value 0 2]
            if {$value eq "x" || $firststr eq "0xx" || $firststr eq "0bx" } {
                set no_x 0
                break
            }
        }
        if {$no_x} {lappend all_facs "$facname"}
    }
'''
        if len(self.signal_list) > 0:
            self.tcl_template = self.tcl_template + f"set ex_facs [list {' '.join(f'{{{item}}}' for item in self.signal_list)}]\ngtkwave::addSignalsFromList $ex_facs\n"
        self.tcl_template = self.tcl_template + "gtkwave::addSignalsFromList $all_facs\ngtkwave::/Time/Zoom/Zoom_Full\n}\n"

    # write tcl script for gtkwave
    def wr_tcl(self):
        template = os.path.join(self.pwd, "add_signal_template.tcl")
        if os.path.exists(template):
            f_temp = open(template, 'r')
            self.tcl_template = f_temp.readlines()
            f_temp.close()
        with open (self.tcl_file, 'w') as f:
            print(self.tcl_file)
            f.write(self.tcl_template)
            for sig in self.waves_list:
                sig = sig.replace('\\', '/')
                f.write(f'gtkwave::loadFile "{sig}" \n')
                f.write("add_sig\n")
            f.write("gtkwave::setTabActive 0")
        return self

    # delete tcl file
    def del_tcl(self):
        if os.path.exists("./add_signal.tcl"):
            os.remove("./add_signal.tcl")
        return self

    # launch gtkwave
    def launch_gtkwave(self):
        cmd = os.path.join(self.pwd, 'gtkwave.exe')
        # cmd = r'E:\gtkwave\gtkwave\bin\gtkwave.exe'
        cmd = cmd + f" -T ./add_signal.tcl"
        subprocess.run(cmd, shell=True, stdout=subprocess.PIPE)
        return self


    def readSignalList(self):
        signal_list_path = os.path.join(pathlib.Path.home(), "signal.gtkw")
        if not os.path.exists(signal_list_path):
            print("no signal.gtkw file")
            return
        with open(signal_list_path, 'r') as f:
            for line in f:
                if re.match(r'^[a-zA-Z]', line):
                    self.signal_list.append(line.strip())



# find *.vcd file from input path
def trace_vcd(p0, p1="", p2=""):
    path = os.path.join(p2,p1,p0)
    if os.path.isdir(path):
        for d in os.listdir(path):
            for ret in trace_vcd(p2=os.path.join(p2,p1), p1=p0, p0=d):
                yield ret
    elif p0.endswith(".vcd") and not p0.startswith("diags"):
        yield [p2, os.path.join(p1,p0)]


def find_vcd(path):
    res = {}
    for d, f in trace_vcd(path):
        if res.get(d):
            res[d].append(f)
        else:
            res[d] = list([f])
    return res



# parse input file or directory
def unpack_file(tar_file):
    if tar_file.endswith(".zip"):
        unpack_path = os.path.join(pathlib.Path.home(), 'OpenSignal')
        try:
            unp = os.path.join(unpack_path, 'temp0')
            if os.path.exists(unp):
                shutil.rmtree(unpack_path)
        except Exception:
            listdir = os.listdir(unpack_path)
            tmp_num = [int(i[4:]) for i in listdir if i.startswith("temp")]
            unp = os.path.join(unpack_path, 'temp' + str(max(tmp_num)+1))
        finally:
            shutil.unpack_archive(tar_file, unp)
        return unp
    else:
        return tar_file


def run():
    if len(sys.argv) == 1:
        try:
            write_key()
            print('Init Over!!')
            input()
        except Exception as e:
            print(e)
            input()
            sys.exit()
    for a in sys.argv[1:]:
        res = find_vcd(unpack_file(a))
        wave_list = []
        for k in res.keys():
            if k:
                os.chdir(k)
            Waves(res[k]).wr_tcl().launch_gtkwave().del_tcl()



if __name__ == "__main__":
    run()
    # a = r'C:\Users\hp\Desktop\test'
    # res = find_vcd(unpack_file(a))
    # wave_list = []
    # for k in res.keys():
    #     if k:
    #         os.chdir(k)
    #     Waves(res[k]).wr_tcl().launch_gtkwave().del_tcl()