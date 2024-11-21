import os
import shutil
from pathlib import Path

def process_images(input_dir, output_dir):
    # Create output directory if it doesn't exist
    os.makedirs(output_dir, exist_ok=True)
    
    # Supported image extensions
    image_extensions = ('.jpg', '.jpeg', '.webp')
    
    # Walk through the input directory
    for root, _, files in os.walk(input_dir):
        for file in files:
            if file.lower().endswith(image_extensions):
                # Get the full path of the file
                file_path = Path(root) / file
                
                # Get relative path from input directory
                rel_path = os.path.relpath(root, input_dir)
                
                # Create new filename with directory structure
                if rel_path != '.':
                    new_filename = f"{rel_path.replace(os.sep, '_')}_{file}"
                else:
                    new_filename = file
                
                # Create destination path
                dest_path = Path(output_dir) / new_filename
                
                # Copy the file
                shutil.copy2(file_path, dest_path)
                print(f"Copied: {file_path} -> {dest_path}")

def main():
    import argparse
    
    parser = argparse.ArgumentParser(description='Copy images from nested directories to a flat output directory')
    parser.add_argument('input_dir', help='Input directory containing nested images')
    parser.add_argument('output_dir', help='Output directory for copied images')
    
    args = parser.parse_args()
    
    process_images(args.input_dir, args.output_dir)

if __name__ == "__main__":
    main()
