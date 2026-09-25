import os
import glob

for folder in ['components', 'scenes']:
    for crate_dir in glob.glob(f'{folder}/*'):
        cargo_path = os.path.join(crate_dir, 'Cargo.toml')
        if os.path.exists(cargo_path):
            with open(cargo_path, 'r') as f:
                content = f.read()
            content = content.replace('path = "../sdk"', 'path = "../../sdk"')
            with open(cargo_path, 'w') as f:
                f.write(content)

