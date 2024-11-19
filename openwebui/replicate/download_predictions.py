import os
import replicate
import requests
import time
from datetime import datetime
from dotenv import load_dotenv

# Load environment variables from .env file
load_dotenv()

# Replicate API token should be set in .env file
if not os.getenv("REPLICATE_API_TOKEN"):
    raise ValueError("Please set REPLICATE_API_TOKEN in .env file")

def download_prediction(prediction):
    """Download the output files from a prediction"""
    if not prediction.output:
        return
    
    # Create downloads directory if it doesn't exist
    os.makedirs("replicate_downloads", exist_ok=True)
    
    # Create subdirectory with timestamp and prediction ID
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    download_dir = f"replicate_downloads/{timestamp}_{prediction.id}"
    os.makedirs(download_dir, exist_ok=True)
    
    # Handle different output types
    if isinstance(prediction.output, str):
        # Single URL or text output
        if prediction.output.startswith('http'):
            response = requests.get(prediction.output)
            filename = prediction.output.split('/')[-1]
            with open(f"{download_dir}/{filename}", 'wb') as f:
                f.write(response.content)
        else:
            # Text output
            with open(f"{download_dir}/output.txt", 'w') as f:
                f.write(prediction.output)
    
    elif isinstance(prediction.output, list):
        # Multiple outputs
        for i, item in enumerate(prediction.output):
            if isinstance(item, str) and item.startswith('http'):
                response = requests.get(item)
                filename = item.split('/')[-1]
                with open(f"{download_dir}/{filename}", 'wb') as f:
                    f.write(response.content)
            else:
                with open(f"{download_dir}/output_{i}.txt", 'w') as f:
                    f.write(str(item))

def main():
    # Get all predictions
    predictions = replicate.predictions.list()
    
    print("Starting download of all predictions...")
    
    for prediction in predictions:
        try:
            print(f"Downloading prediction {prediction.id}...")
            download_prediction(prediction)
            time.sleep(1)  # Add small delay to avoid rate limiting
        except Exception as e:
            print(f"Error downloading prediction {prediction.id}: {str(e)}")
    
    print("Download complete!")

if __name__ == "__main__":
    main()
